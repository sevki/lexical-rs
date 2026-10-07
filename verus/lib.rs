//! Formal verification of the pure algorithms behind `lexical-core` and `lexical-adw`.
//! Not part of the Cargo workspace: verified with `verus/verify.sh`.
//!
//! Verus cannot check the production crates directly (they use `HashMap`, trait objects,
//! `Rc<RefCell<..>>` and GTK), so each module is an executable model that mirrors one
//! function of the production code line by line. `verus/README.md` lists the mapping.
#![allow(unused_imports)]

pub mod diff;
pub mod history;
pub mod layout_sync;
pub mod split;
