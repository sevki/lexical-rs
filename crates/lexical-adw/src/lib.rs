//! libadwaita / GTK4 views for [`lexical_core`].
//!
//! * [`LexicalView`] – the editing surface (a `GtkTextView` driven by the engine)
//! * [`Toolbar`] – formatting controls built from GTK/libadwaita widgets
//! * [`reconciler`] – applies a core [`Layout`](lexical_core::Layout) to a `GtkTextBuffer`

pub mod reconciler;
mod toolbar;
mod view;

pub use adw;
pub use lexical_core as core;
pub use toolbar::Toolbar;
pub use view::LexicalView;
