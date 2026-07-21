//! Library target for `nwg-notifications`.
//!
//! This is a binary-first project; the library target exists so the
//! integration suite in `tests/` has a linkable seam. It is **not a
//! public API**: everything is `#[doc(hidden)]`, semver guarantees do
//! not apply to the library surface, and items may change or vanish in
//! any release. Use the binary, not this library.

mod app;
mod config;
mod config_file;
mod dbus;
mod listeners;
mod notification;
mod paths;
mod persistence;
mod state;
mod ui;
mod waybar;

#[doc(hidden)]
pub use app::run;
