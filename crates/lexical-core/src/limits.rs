//! Optional editing limits. Nothing is capped by default.
//!
//! Lexical itself has no built-in cap on list nesting or block indentation (its
//! playground opts in with a `ListMaxIndentLevelPlugin`); hosts that want one set it here.

/// Caps applied by the `Indent` command. `None` means unlimited.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Limits {
    /// Deepest nesting level (0 = top level) a list item can be indented to.
    pub max_list_depth: Option<u32>,
    /// Highest `indent` level for non-list blocks.
    pub max_indent: Option<u32>,
}
