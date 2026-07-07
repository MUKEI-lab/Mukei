//! FFI adapter layer.
//!
//! The `mukei-core` crate compiles *without* linking to `cxx-qt`. The
//! bridge crate re-exposes these types as `QObject`s. Any value that
//! crosses the boundary **must** go through one of these adapters so
//! we have a single, auditable conversion path.
//!
//! # Invariants
//!
//! - Every type re-exported from here is JSON-stable or `#[repr(C)]`
//!   compatible. Adding non-stable types is a contract break (QML
//!   reads these shapes directly).
//! - There is exactly **one** [`CallbackGuard`](crate::guard::CallbackGuard)
//!   implementation in the workspace. `mukei-ffi-shim` consumes it via
//!   `callback_with_guard!` instead of reimplementing the generation
//!   counter.
//! - Every FFI callback dispatch is wrapped in
//!   `std::panic::catch_unwind`. This depends on workspace-wide
//!   `panic = "unwind"` (see the comment at the top of `rust/Cargo.toml`).
//!
//! TRD §1.2 / §1.3 / §1.5.

pub mod agent;
pub mod callback;
pub mod tags;

pub use agent::FfiAgentSnapshot;
pub use callback::{FfiCallbackRegistration, FfiStateChange};
