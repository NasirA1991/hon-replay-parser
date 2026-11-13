//! # honreplay
//!
//! A complete parser for Heroes of Newerth replay files (`.honreplay`, S2R2 v6/v7).
//!
//! ```no_run
//! let replay = honreplay::Replay::open("match.honreplay")?;
//! let mut frames = replay.frames();
//! while let Some(frame) = frames.next_frame() {
//!     let frame = frame?;
//!     for change in &frame.changes { /* entity created / updated / deleted */ }
//! }
//! # Ok::<(), honreplay::Error>(())
//! ```
pub mod analysis;
pub mod archive;
pub mod bits;
pub mod entity;
pub mod error;
pub mod events;
pub mod header;
pub mod messages;
pub mod replay;
pub mod replayinfo;
pub mod schema;

pub use analysis::{Options, Report, analyze};
pub use entity::{Change, Entity, Pool};
pub use error::{Error, Result};
pub use header::Header;
pub use replay::{Frame, FrameReader, Replay};
pub use replayinfo::ReplayInfo;
pub use schema::Value;
