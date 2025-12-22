mod chrom;
mod d4file;
mod dict;
mod header;
#[cfg(all(feature = "mapped_io", not(target_arch = "wasm32")))]
pub mod ptab;

pub mod stab;
#[cfg(all(feature = "task", not(target_arch = "wasm32")))]
pub mod task;

pub mod ssio;

pub mod index;

pub use chrom::Chrom;

// Reader-only exports (available with mapped_io)
#[cfg(all(feature = "mapped_io", not(target_arch = "wasm32")))]
pub use d4file::{find_tracks, find_tracks_in_file, D4TrackReader};

// Task-related exports (available with task feature)
#[cfg(all(feature = "task", not(target_arch = "wasm32")))]
pub use d4file::{D4MatrixReader, MultiTrackReader};

// Writer-related exports (available with writer feature)
#[cfg(all(feature = "writer", not(target_arch = "wasm32")))]
pub use d4file::{D4FileBuilder, D4FileMerger, D4FileWriter, D4FileWriterExt};

pub use dict::Dictionary;

pub use header::Header;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
