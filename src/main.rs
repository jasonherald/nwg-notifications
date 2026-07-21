//! Binary shim. All coordinator logic lives in the library's `app`
//! module so the integration suite in `tests/` can link the same code —
//! see `src/lib.rs` for the no-public-API disclaimer on the library
//! target.

fn main() {
    nwg_notifications::run();
}
