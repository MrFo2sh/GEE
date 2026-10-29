//! GAE core: everything an environment needs that does not need a renderer.
//!
//! - `model`: the event schema every source speaks, and the `World` reducer that applies it.
//! - `layout`: orbital positions (core at the origin, one ring per depth).
//! - `game`: XP, levels, streaks and achievements, as pure rules over world events.
//! - `spec`: how a domain describes itself (names, kinds, palette, rules).
//! - `source`, `tail`, `ingest`, `bridge`: how events get in, on worker threads.
//!
//! Rule: nothing under this crate imports Bevy, so it compiles and tests without a GPU.

pub mod bridge;
pub mod game;
pub mod ingest;
pub mod layout;
pub mod model;
pub mod source;
pub mod spec;
pub mod tail;

pub use model::{Event, Severity, World, now_millis};
pub use source::Source;
pub use spec::{KindStyle, Shape, Spec};
