# MQTT Investigation Notes

## Findings

### Discovery API
- Endpoint: https://internal.cloud.remarkable.com/discovery/v1/endpoints
- Response:
  - notifications: eu.tectonic.remarkable.com
  - webapp: webapp-prod.cloud.remarkable.engineering
  - mqttbroker: vernemq-prod.cloud.remarkable.engineering

### WebSocket Connection
- URL: wss://eu.tectonic.remarkable.com/notifications/ws/json/1
- Auth: Bearer token in Authorization header
- Connection succeeds but immediately closes after CONNECT packet
- May need specific MQTT client ID format or MQTT version

### Binary Analysis (xochitl 3.3.2.1666)
- Uses Paho MQTT C++ (mqttpp) v. 1.2
- ScreenShareMqtt class handles screen share MQTT
- MQTT topic subscription: "Subscribing to topic {:q}..."
- VerneMQ URL pattern: vernemq-%1.cloud.remarkable.engineering

### Token Structure
- UserToken: ES256 signed, 3-hour lifetime
- Scopes include "screenshare" for MQTT access
- Tectonic region: eu (used for endpoint selection)

### Next Steps
1. Capture real device MQTT traffic (need tcpdump on device or MITM proxy)
2. Analyze MQTT client ID format from xochitl binary
3. Determine MQTT version (3.1.1 vs 5.0)
4. Find topic naming conventions (likely user/{userId}/client/...)
