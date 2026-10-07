//! The Lexical editor as a *domain* for the generic kernels (`crate::kernels`).
//!
//! The model abstracts `lexical_core::EditorState` to what the document invariants talk
//! about: blocks (kind, indent), their characters (code point + format bits) and a
//! range selection (anchor/focus as block index + char offset). Inline node structure
//! (runs, links) is a representation of the same character sequence; the production
//! normalization (merge adjacent equal-format runs, drop empty nodes) is checked at
//! runtime by `EditorState::check_invariants` and the trace tests in
//! `crates/lexical-core/tests/invariants.rs`.
//!
//! Commands mirror `lexical_core::Command`; each spec function mirrors the like-named
//! method in `crates/lexical-core/src/{edit,blocks}/`.
//!
//! Domain obligations discharged here (`impl Domain for EditorDomain`):
//!   (R1) the initial document (one empty paragraph, caret in it) satisfies `inv`,
//!   (R2) `inv(d) ==> inv(normalize(apply(d, cmd)))` for every command.
//!
//! Intent properties (the "delta laws" of the editor), proved per command:
//!   * Insert adds exactly the inserted characters,
//!   * deleting removes exactly the selected characters (one for Backspace),
//!   * Enter (which branches on the block kind, like `insert_paragraph`): in an ordinary
//!     block it conserves every character and adds exactly one block; in a code block it
//!     inserts one line break; in an empty list item it outdents,
//!   * kind / indent / format / selection commands conserve every character and its
//!     code point (formatting changes only format bits).
//!
//! Indentation is unbounded in the model, as it is in production by default. Optional
//! host limits (`lexical_core::Limits`) only turn `Indent` into a no-op at the cap, which
//! cannot break any property stated here.
//!
//! Layout: `model` (types, invariant, positions), `counting` (character counts),
//! `delete`, `insert`, `enter`, `blocks` (kind/indent/format maps), `commands`
//! (the command set and the `Domain` instance).

mod blocks;
mod commands;
mod counting;
mod delete;
mod enter;
mod insert;
mod model;

pub use blocks::*;
pub use commands::*;
pub use counting::*;
pub use delete::*;
pub use enter::*;
pub use insert::*;
pub use model::*;
