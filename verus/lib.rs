//! Formal verification of `lexical-rs`, in the style of
//! [dafny-replay](https://github.com/metareflection/dafny-replay): generic *kernels*
//! (replay/undo-redo, server authority/sync) are proved once against a *domain*
//! interface, and the Lexical editor is a domain that discharges the obligations.
//! Not part of the Cargo workspace: verified with `verus/verify.sh`.
//!
//! Verus cannot check the production crates directly (they use `HashMap`, trait objects,
//! `Rc<RefCell<..>>` and GTK), so each module is a model that mirrors the production
//! code; `verus/GUARANTEES.md` states exactly what is proved, what is obligated and what
//! is trusted.
#![allow(unused_imports)]

pub mod bridge;
pub mod domains;
pub mod kernels;
pub mod production_history;
pub mod theorems;
