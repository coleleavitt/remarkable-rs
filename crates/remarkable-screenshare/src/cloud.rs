//! Cloud signaling: connect to a self-hosted remarkable-server screenshare broker
//! (e.g. remarkable.unwrap.rs:8883) over mqtt/TLS, join the tablet's active
//! screenshare room, and negotiate WebRTC so frames flow over the internet
//! instead of USB.
//!
//! Wire protocol (verified against a live tablet):
//!   connect: client_id = username = CID, password = user token
//!   subscribe: user/{uid}/#
//!   publish to: remarkable/screenshare/signaling/user/{uid}/client/{CID}/signaling
//!     1. {"type":"join-active-room","room":"","roomId":""}
//!        <- {"type":"room-joined","roomId":R,...}   (or {"type":"room-not-found"})
//!     2. {"type":"broadcast","roomId":R,"payload":{"type":"request-offer","id":"X"}}
//!        <- {"type":"direct","clientId":T,"payload":{"type":"webtrc","payload":{"type":"offer","description":SDP}}}
//!        <- {"type":"direct","clientId":T,"payload":{"type":"webtrc","payload":{"type":"candidate","candidate":"...","mid":"0"}}}
//!     3. we reply with {"type":"direct","roomId":R,"clientId":T,"payload":{"type":"webtrc","payload":{"type":"answer","description":SDP}}}
//!        and our own candidates the same way.
//! ("webtrc" is xochitl's own spelling.)

use std::time::Duration;

use rumqttc::{AsyncClient, Event, MqttOptions, Packet, QoS, Transport};
use serde_json::{json, Value};
use tracing::{debug, info, warn};

use crate::error::{Error, Result};
use std::sync::Arc;
use crate::webrtc::WebRtcHandler;

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
    /// Extra ICE servers (STUN/TURN URLs). Empty = host candidates only (same LAN).
    pub ice_servers: Vec<String>,
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

/// Connect to the broker, join the active room, and complete WebRTC negotiation.
pub async fn connect(cfg: CloudConfig) -> Result<CloudSession> {
    let cid = format!("viewer-{}", &uuid::Uuid::new_v4().simple().to_string()[..12]);
    let uid = cfg.user_id.clone();
    let pub_topic = format!("remarkable/screenshare/signaling/user/{uid}/client/{cid}/signaling");

    let mut opts = MqttOptions::new(cid.clone(), cfg.host.clone(), cfg.port);
    opts.set_credentials(cid.clone(), cfg.user_token.clone());
    opts.set_keep_alive(Duration::from_secs(30));
    opts.set_max_packet_size(1 << 20, 1 << 20);
    opts.set_transport(Transport::tls_with_default_config());

    info!("cloud: connecting to {}:{} as {}", cfg.host, cfg.port, cid);
    let (client, mut eventloop) = AsyncClient::new(opts, 64);

    // --- Phase 1: connect, join room, collect offer + early candidates ---
    let deadline = tokio::time::Instant::now() + cfg.timeout;
    let mut room_id: Option<String> = None;
    let mut tablet_cid: Option<String> = None;
    let mut offer_sdp: Option<String> = None;
    let mut early_cands: Vec<(String, Option<String>)> = Vec::new();

    let join = json!({"type":"join-active-room","room":"","roomId":""}).to_string();

    while offer_sdp.is_none() {
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
                client.subscribe(format!("user/{uid}/#"), QoS::AtLeastOnce).await.map_err(sig_err)?;
                client.publish(&pub_topic, QoS::AtLeastOnce, false, join.clone()).await.map_err(sig_err)?;
            }
            Event::Incoming(Packet::Publish(p)) => {
                let Ok(j) = serde_json::from_slice::<Value>(&p.payload) else { continue };
                match j["type"].as_str().unwrap_or("") {
                    "room-not-found" => {
                        return Err(Error::DeviceNotReady);
                    }
                    "room-joined" if room_id.is_none() => {
                        let r = j["roomId"].as_str().unwrap_or_default().to_string();
                        info!("cloud: joined room {r}; requesting offer");
                        let req = json!({"type":"broadcast","roomId":r,
                                         "payload":{"type":"request-offer","id":cid}}).to_string();
                        client.publish(&pub_topic, QoS::AtLeastOnce, false, req).await.map_err(sig_err)?;
                        room_id = Some(r);
                    }
                    "direct" => {
                        if let Some(t) = j["clientId"].as_str() { tablet_cid.get_or_insert(t.to_string()); }
                        let inner = &j["payload"]["payload"];
                        match inner["type"].as_str().unwrap_or("") {
                            "offer" => {
                                offer_sdp = inner["description"].as_str().map(String::from);
                                info!("cloud: got offer from tablet");
                            }
                            "candidate" => {
                                if let Some(c) = inner["candidate"].as_str() {
                                    early_cands.push((c.to_string(), inner["mid"].as_str().map(String::from)));
                                }
                            }
                            other => debug!("cloud: ignoring direct inner type {other}"),
                        }
                    }
                    other => debug!("cloud: ignoring {other} on {}", p.topic),
                }
            }
            _ => {}
        }
    }

    let room_id = room_id.ok_or_else(|| sig_err("offer without room"))?;
    let tablet_cid = tablet_cid.ok_or_else(|| sig_err("offer without clientId"))?;
    let offer = offer_sdp.unwrap();

    // --- Phase 2: WebRTC answer ---
    let stun = if cfg.ice_servers.is_empty() { Some(vec![]) } else { Some(cfg.ice_servers.clone()) };
    let (webrtc, mut ice_rx, data_rx) = WebRtcHandler::new(stun).await?;
    let webrtc = Arc::new(webrtc);
    let answer = webrtc.accept_offer(&offer).await?;

    let (room_id_w, tablet_cid_w) = (room_id.clone(), tablet_cid.clone());
    let wrap = move |inner: Value| json!({"type":"direct","roomId":room_id_w,"clientId":tablet_cid_w,
                                      "payload":{"type":"webtrc","payload":inner}}).to_string();
    client.publish(&pub_topic, QoS::AtLeastOnce, false,
                   wrap(json!({"type":"answer","description":answer}))).await.map_err(sig_err)?;
    info!("cloud: sent answer");

    for (c, mid) in early_cands.drain(..) {
        if let Err(e) = webrtc.add_ice_candidate(&c, mid.as_deref(), Some(0)).await {
            warn!("cloud: bad early candidate: {e}");
        }
    }

    // --- Phase 3: keep trickling ICE both ways in the background ---
    let wrtc_cands = Arc::clone(&webrtc);
    let task_client = client.clone();
    let task_topic = pub_topic.clone();
    let signaling_task = tokio::spawn(async move {
        loop {
            tokio::select! {
                ev = eventloop.poll() => match ev {
                    Ok(Event::Incoming(Packet::Publish(p))) => {
                        let Ok(j) = serde_json::from_slice::<Value>(&p.payload) else { continue };
                        let inner = &j["payload"]["payload"];
                        if j["type"] == "direct" && inner["type"] == "candidate" {
                            if let Some(c) = inner["candidate"].as_str() {
                                let _ = wrtc_cands.add_ice_candidate(c, inner["mid"].as_str(), Some(0)).await;
                            }
                        }
                    }
                    Ok(_) => {}
                    Err(e) => { warn!("cloud: signaling connection ended: {e}"); break; }
                },
                Some(c) = ice_rx.recv() => {
                    let msg = wrap(json!({"type":"candidate","candidate":c.candidate,
                                          "mid":c.sdp_mid.unwrap_or_else(|| "0".into())}));
                    let _ = task_client.publish(&task_topic, QoS::AtLeastOnce, false, msg).await;
                }
            }
        }
    });

    Ok(CloudSession { webrtc, data_rx, signaling_task })
}
