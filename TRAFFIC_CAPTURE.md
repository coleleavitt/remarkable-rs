# Traffic Capture Guide for reMarkable Protocol Analysis

## Overview

Two protocol elements require traffic capture from a live device:

1. **Upload API** - Requires a checksum header in unknown format
2. **MQTT Real-time** - VerneMQ authentication format unknown

## Prerequisites

- Device connected to USB (10.11.99.1) or WiFi
- SSH access to device (root password = developer password)
- Fresh UserToken (valid for 3 hours)

## Setup Traffic Capture

### Method 1: tcpdump on device

```bash
# SSH to device
ssh root@10.11.99.1

# Capture all HTTPS traffic
tcpdump -i wlan0 -w /tmp/capture.pcap port 443

# After sync operation, copy to host
scp root@10.11.99.1:/tmp/capture.pcap ./
```

### Method 2: mitmproxy on host

```bash
# Install mitmproxy
pip install mitmproxy

# Start proxy
mitmproxy --mode transparent --showhost

# On device, set proxy via SSH
ssh root@10.11.99.1
export https_proxy=http://<host-ip>:8080
```

### Method 3: Modify device /etc/hosts

```bash
# SSH to device
ssh root@10.11.99.1

# Redirect traffic through proxy
echo "<host-ip> eu.tectonic.remarkable.com" >> /etc/hosts
```

## Trigger Sync Operations

### Upload (for checksum header)

```bash
# On device
ssh root@10.11.99.1

# Create a test document change
touch /home/root/.local/share/remarkable/xochitl/<doc-id>/*.rm

# Restart xochitl to trigger sync
systemctl restart xochitl
```

### MQTT Connection (for auth format)

```bash
# MQTT is initiated automatically when xochitl starts
# Watch for connections to vernemq-prod.eu.remarkable.engineering:443
```

## Analyze Capture

```bash
# Extract HTTP requests
tshark -r capture.pcap -Y "http" -T json

# Look for sync/v3 upload endpoints
tshark -r capture.pcap -Y "http.request.uri contains sync" -T fields -e http.request.full_uri -e http.request.method

# Extract headers
tshark -r capture.pcap -Y "http.request.uri contains upload" -T fields -e http.request.line
```

## Expected Findings

### Upload API
- Endpoint: `PUT https://eu.tectonic.remarkable.com/sync/v3/files/{hash}`
- Headers needed:
  - `Authorization: Bearer {UserToken}`
  - `rm-filename: {filename}` (documented)
  - `rm-checksum: {???}` (unknown format - capture this)

### MQTT
- Endpoint: `wss://vernemq-prod.eu.remarkable.engineering/mqtt`
- Auth mechanism unknown - may be:
  - Username/password in CONNECT packet
  - Custom WebSocket header
  - Client certificate

## Files to Capture

1. `upload_headers.txt` - Full HTTP headers from upload request
2. `mqtt_handshake.pcap` - WebSocket upgrade and MQTT CONNECT
3. `checksum_examples.txt` - Multiple checksum values with corresponding file hashes

## Integration

Once captured, update:

1. `remarkable-sync/src/client.rs` - Add upload method with correct checksum
2. `remarkable-mqtt/src/lib.rs` - Implement correct auth mechanism
