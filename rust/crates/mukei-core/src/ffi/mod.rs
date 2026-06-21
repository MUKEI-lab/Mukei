//! FFI adapter layer.
//!
//! The `mukei-core` crate compiles *without* linking to `cxx-qt`. The
//! bridge crate re-exposes these types as `QObject`s. Any value that
//! crosses the boundary **must** go through one of these adapters so
//! we have a single, auditable conversion path.
//!
//! TRD §1.2 / §1.3 / §1.5.

pub mod agent;
pub mod callback;
pub mod tags;

pub use agent::FfiAgentSnapshot;
pub use callback::{FfiCallbackRegistration, FfiStateChange};
