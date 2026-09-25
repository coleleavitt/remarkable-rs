//! Cloud signaling: connect to a self-hosted remarkable-server screenshare broker
//! (e.g. remarkable.unwrap.rs:8883) over mqtt/TLS, join the tablet's active
//! screenshare room, and negotiate WebRTC so frames flow over the internet
//! instead of USB.
//!
//! Wire protocol (verified against a live tablet):
//!   connect: client_id = username = CID, password = user token
//!   subscribe: user/{uid}/signaling and user/{uid}/client/{CID}/signaling/#
//!   publish to: remarkable/screenshare/signaling/user/{uid}/client/{CID}
//!   (the topics xochitl's mqttbroker.cpp uses; message types live in
//!   `remarkable_mqtt::screenshare`)
//!     1. {"type":"join-active-room","room":"","roomId":""}
//!        <- {"type":"room-joined","roomId":R,...}   (or {"type":"room-not-found"})
//!     2. {"type":"broadcast","roomId":R,"payload":{"type":"request-offer","id":"X"}}
//!        <- {"type":"direct","clientId":T,"payload":{"type":"webtrc","payload":{"type":"offer","description":SDP}}}
//!        <- {"type":"direct","clientId":T,"payload":{"type":"webtrc","payload":{"type":"candidate","candidate":"...","mid":"0"}}}
//!     3. we reply with {"type":"direct","roomId":R,"clientId":T,"payload":{"type":"webtrc","payload":{"type":"answer","description":SDP}}}
//!        and our own candidates the same way.
//! ("webtrc" is xochitl's own spelling.)

use std::sync::Arc;
use std::time::Duration;

use remarkable_mqtt::screenshare::{signaling_topic, subscriptions};
use remarkable_mqtt::{PeerMessage, SignalingEvent, SignalingRequest, WebRtcMessage};
use rumqttc::{AsyncClient, Event, MqttOptions, Packet, QoS, Transport};
use tracing::{debug, info, warn};

use crate::error::{Error, Result};
use crate::webrtc::{IceServer, TransportConfig, WebRtcHandler};

/// Settings for cloud (internet) screenshare.
#[derive(Debug, Clone)]
pub struct CloudConfig {
    /// Broker host, e.g. "remarkable.unwrap.rs"
    pub host: String,
    /// Broker TLS port (8883)
    pub port: u16,
    /// User token (JWT) from /token/json/2/user/new on the server
    pub user_token: String,
    /// User id used in topics (JWT `sub`/user id claim; "local-user" on remarkable-server)
    pub user_id: String,
    /// Local WebRTC setup (ICE servers, UDP port range).
    pub transport: TransportConfig,
    /// How long to wait for the tablet's offer
    pub timeout: Duration,
}

/// Result of a successful cloud negotiation.
pub struct CloudSession {
    pub webrtc: Arc<WebRtcHandler>,
    pub data_rx: tokio::sync::mpsc::UnboundedReceiver<Vec<u8>>,
    /// Keeps the mqtt event loop alive (trickle ICE) for the session lifetime.
    pub signaling_task: tokio::task::JoinHandle<()>,
}

fn sig_err(e: impl std::fmt::Display) -> Error {
    Error::MqttSignaling(e.to_string())
}

/// Publishes signaling requests for one viewer.
#[derive(Clone)]
struct Signaler {
    client: AsyncClient,
    topic: String,
}

impl Signaler {
    async fn send(&self, request: &SignalingRequest) -> Result<()> {
        let body = request.to_bytes().map_err(sig_err)?;
        self.client.publish(&self.topic, QoS::AtLeastOnce, false, body).await.map_err(sig_err)
    }

    /// Send a WebRTC message straight to the tablet.
    async fn to_tablet(&self, room_id: &str, tablet: &str, msg: WebRtcMessage) -> Result<()> {
        self.send(&SignalingRequest::Direct {
            room_id: room_id.to_owned(),
            client_id: tablet.to_owned(),
            payload: PeerMessage::WebRtc { payload: msg },
        })
        .await
    }
}

/// Connect to the broker, join the active room, and complete WebRTC negotiation.
pub async fn connect(cfg: CloudConfig) -> Result<CloudSession> {
    let cid = format!("viewer-{}", &uuid::Uuid::new_v4().simple().to_string()[..12]);
    let uid = cfg.user_id.clone();

    let mut opts = MqttOptions::new(cid.clone(), cfg.host.clone(), cfg.port);
    opts.set_credentials(cid.clone(), cfg.user_token.clone());
    opts.set_keep_alive(Duration::from_secs(30));
    opts.set_max_packet_size(1 << 20, 1 << 20);
    opts.set_transport(Transport::tls_with_default_config());

    info!("cloud: connecting to {}:{} as {}", cfg.host, cfg.port, cid);
    let (client, mut eventloop) = AsyncClient::new(opts, 64);
    let signaler = Signaler { client: client.clone(), topic: signaling_topic(&uid, &cid) };

    // --- Phase 1: connect, join room, collect offer + early candidates ---
    let deadline = tokio::time::Instant::now() + cfg.timeout;
    let mut room_id: Option<String> = None;
    let mut broker_ice: Vec<IceServer> = Vec::new();
    // Candidates can arrive before the offer; keep their sender to match later.
    let mut early_cands: Vec<(String, String, Option<String>)> = Vec::new();

    let (tablet_cid, offer) = loop {
        let ev = tokio::time::timeout_at(deadline, eventloop.poll())
            .await
            .map_err(|_| Error::Timeout(match &room_id {
                None => "no active screenshare room (is screen sharing on in the tablet?)".into(),
                Some(_) => "joined room but tablet never sent an offer".into(),
            }))?
            .map_err(|e| Error::MqttConnection(e.to_string()))?;

        match ev {
            Event::Incoming(Packet::ConnAck(ack)) => {
                info!("cloud: connected ({:?})", ack.code);
                for filter in subscriptions(&uid, &cid) {
                    client.subscribe(filter, QoS::AtLeastOnce).await.map_err(sig_err)?;
                }
                signaler
                    .send(&SignalingRequest::JoinActiveRoom { room: String::new(), room_id: String::new() })
                    .await?;
            }
            Event::Incoming(Packet::Publish(p)) => match SignalingEvent::from_bytes(&p.payload) {
                Some(SignalingEvent::RoomNotFound) => return Err(Error::DeviceNotReady),
                Some(SignalingEvent::RoomJoined { room_id: r, ice_servers }) if room_id.is_none() => {
                    info!("cloud: joined room {r}; requesting offer");
                    broker_ice = ice_servers.as_ref().map(parse_ice_servers).unwrap_or_default();
                    signaler
                        .send(&SignalingRequest::Broadcast {
                            room_id: r.clone(),
                            payload: PeerMessage::RequestOffer { id: cid.clone() },
                        })
                        .await?;
                    room_id = Some(r);
                }
                Some(SignalingEvent::Direct { client_id, payload: PeerMessage::WebRtc { payload } }) if room_id.is_some() => {
                    match payload {
                        WebRtcMessage::Offer { description } => {
                            info!("cloud: got offer from tablet {client_id}");
                            break (client_id, description);
                        }
                        WebRtcMessage::Candidate { candidate, mid } => early_cands.push((client_id, candidate, mid)),
                        WebRtcMessage::Answer { .. } => debug!("cloud: ignoring answer"),
                    }
                }
                other => debug!("cloud: ignoring {other:?} on {}", p.topic),
            },
            _ => {}
        }
    };

    let room_id = room_id.ok_or_else(|| sig_err("offer without room"))?;

    // --- Phase 2: WebRTC answer ---
    // The broker's STUN/TURN servers (room-joined) come on top of our own.
    let mut transport = cfg.transport.clone();
    transport.ice_servers.extend(broker_ice);
    let (webrtc, mut ice_rx, data_rx) = WebRtcHandler::new(transport).await?;
    let webrtc = Arc::new(webrtc);
    let answer = webrtc.accept_offer(&offer).await?;

    signaler.to_tablet(&room_id, &tablet_cid, WebRtcMessage::Answer { description: answer }).await?;
    info!("cloud: sent answer");

    // Only the peer that made the offer is part of this session.
    for (_, c, mid) in early_cands.into_iter().filter(|(from, ..)| *from == tablet_cid) {
        if let Err(e) = webrtc.add_ice_candidate(&c, mid.as_deref(), Some(0)).await {
            warn!("cloud: bad early candidate: {e}");
        }
    }

    // --- Phase 3: keep trickling ICE both ways in the background ---
    let wrtc_cands = Arc::clone(&webrtc);
    let signaling_task = tokio::spawn(async move {
        loop {
            tokio::select! {
                ev = eventloop.poll() => match ev {
                    Ok(Event::Incoming(Packet::Publish(p))) => {
                        if let Some(SignalingEvent::Direct {
                            client_id,
                            payload: PeerMessage::WebRtc { payload: WebRtcMessage::Candidate { candidate, mid } },
                        }) = SignalingEvent::from_bytes(&p.payload)
                        {
                            if client_id != tablet_cid { continue }
                            let _ = wrtc_cands.add_ice_candidate(&candidate, mid.as_deref(), Some(0)).await;
                        }
                    }
                    Ok(_) => {}
                    Err(e) => { warn!("cloud: signaling connection ended: {e}"); break; }
                },
                Some(c) = ice_rx.recv() => {
                    let msg = WebRtcMessage::Candidate {
                        candidate: c.candidate,
                        mid: Some(c.sdp_mid.unwrap_or_else(|| "0".into())),
                    };
                    let _ = signaler.to_tablet(&room_id, &tablet_cid, msg).await;
                }
            }
        }
    });

    Ok(CloudSession { webrtc, data_rx, signaling_task })
}

/// The broker's ICE list from `room-joined`: `{"ice_servers": [...]}` whose
/// entries carry `url` (xochitl's spelling) or `urls`, plus TURN credentials.
fn parse_ice_servers(v: &serde_json::Value) -> Vec<IceServer> {
    let list = v.get("ice_servers").unwrap_or(v);
    let Some(entries) = list.as_array() else { return Vec::new() };
    entries
        .iter()
        .filter_map(|e| {
            let urls: Vec<String> = match e.get("urls").or_else(|| e.get("url"))? {
                serde_json::Value::String(u) => vec![u.clone()],
                serde_json::Value::Array(a) => a.iter().filter_map(|u| u.as_str().map(String::from)).collect(),
                _ => return None,
            };
            let text = |k: &str| e.get(k).and_then(|x| x.as_str()).unwrap_or_default().to_owned();
            (!urls.is_empty()).then(|| IceServer { urls, username: text("username"), credential: text("credential") })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn broker_ice_servers_are_parsed() {
        let v = serde_json::json!({"ice_servers": [
            {"url": "stun:stun.example:3478"},
            {"urls": ["turn:turn.example:3478"], "username": "u", "credential": "p"},
            {"nope": true}
        ]});
        assert_eq!(parse_ice_servers(&v), vec![
            IceServer::url("stun:stun.example:3478"),
            IceServer { urls: vec!["turn:turn.example:3478".into()], username: "u".into(), credential: "p".into() },
        ]);
        assert!(parse_ice_servers(&serde_json::json!([])).is_empty());
    }
}
