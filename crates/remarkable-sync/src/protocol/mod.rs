//! Sync protocol version abstraction
//!
//! Supports multiple sync protocol versions:
//! - V1: Original document-storage API (firmware 1.x-2.x)
//! - V1_5: Transitional with batch operations  
//! - V2: Batch sync with signed URLs
//! - V3: Current hash-tree based (tectonic)
//! - V4: Merkle tree with generation counters

mod version;
mod traits;
mod v1;
mod v1_5;
mod v2;
mod v3;
mod v4;
mod detection;
mod unified;

pub use version::*;
pub use traits::*;
pub use v1::*;
pub use v1_5::*;
pub use v2::*;
pub use v3::*;
pub use v4::*;
pub use detection::*;
pub use unified::*;
