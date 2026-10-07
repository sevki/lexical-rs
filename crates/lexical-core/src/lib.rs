//! Toolkit-agnostic port of the Lexical rich text editor engine (modelled on
//! Lexical iOS): node tree, editor state, range selection, update pipeline,
//! commands, history, Lexical JSON, and a flattened layout for text widgets.

pub mod blocks;
mod content;
pub mod edit;
pub mod editor;
pub mod error;
pub mod format;
pub mod history;
mod invariants;
pub mod json;
pub mod layout;
pub mod limits;
pub mod markdown;
pub mod node;
mod normalize;
pub mod selection;
pub mod state;
#[cfg(feature = "jetstream")]
pub mod wire;

pub use blocks::BlockType;
pub use editor::{Command, Editor, ListenerId, Plugin, Tag, UpdateEvent};
pub use error::{Error, Result};
pub use format::{Align, TextFormat};
pub use layout::{BlockStyle, Layout, Line, Run};
pub use limits::Limits;
pub use markdown::MarkdownShortcutsPlugin;
pub use node::{HeadingTag, ListType, Node, NodeData, NodeKey, NodeType, ROOT_KEY};
pub use selection::{Point, PointKind, Selection};
pub use state::EditorState;
