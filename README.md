# remarkable-rs

A comprehensive Rust library for interacting with reMarkable e-paper tablets.

## Crates

| Crate | Description |
|-------|-------------|
| `remarkable-core` | Core types: Document, Stroke, Point, CRDT operations |
| `remarkable-lines` | .rm file parsing (v3/v5/v6) and writing |
| `remarkable-sync` | Cloud sync API client |
| `remarkable-mqtt` | MQTT notifications client |
| `remarkable-pdf` | PDF annotation overlay |
| `remarkable-usb` | USB WebUI client |
| `remarkable-dbus` | D-Bus device control |
| `remarkable-screenshare` | Screen share: RFB v2 protocol, WebRTC transport, frame session |
| `remarkable-firmware` | Firmware extraction (CrAU/SWU) |
| `remarkable-waveform` | Waveform format parsing |
| `remarkable-template` | Template management |
| `remarkable-text` | Text/CRDT handling |
| `remarkable-epub` | EPUB annotations |
| `remarkable-graphql` | GraphQL schema client |
| `remarkable-cli` | Command-line interface |

## Installation

```toml
[dependencies]
remarkable-core = { path = "remarkable-core" }
remarkable-lines = { path = "remarkable-lines" }
remarkable-sync = { path = "remarkable-sync" }
```

## Usage

### Parse .rm files

```rust
use remarkable_lines::parse_rm_file;

let doc = parse_rm_file("notebook.rm")?;
for layer in &doc.layers {
    for stroke in &layer.strokes {
        println!("Stroke with {} points", stroke.points.len());
    }
}
```

### Sync with cloud

```rust
use remarkable_sync::SyncClient;

let client = SyncClient::from_token_files(
    "device_token.txt",
    "user_token.txt"
)?;

// Download all documents
let docs = client.list_documents().await?;
for doc in docs {
    client.download_document(&doc.id, "./backup/").await?;
}
```

### Export to SVG

```rust
use remarkable_lines::{parse_rm_file, export_svg};

let doc = parse_rm_file("notebook.rm")?;
export_svg(&doc, "output.svg")?;
```

## CLI

```bash
# Parse and display .rm file info
remarkable info -f notebook.rm

# Export to SVG
remarkable parse -f notebook.rm -o output.svg

# Sync operations
remarkable sync list
remarkable sync pull doc-id ./output/
remarkable backup ./backup-dir/
```

## Protocol Documentation

See the companion research repository for full protocol documentation:
https://github.com/coleleavitt/remarkable-research

## License

MIT License
