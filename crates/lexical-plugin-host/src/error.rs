use std::fmt;

#[derive(Debug)]
pub enum PluginError {
    /// The bytes are not a valid component or do not implement `lexical:editor/plugin`.
    Load(String),
    /// The plugin trapped, ran out of fuel or memory, or broke the interface contract.
    Call(String),
}

pub type Result<T> = std::result::Result<T, PluginError>;

impl fmt::Display for PluginError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PluginError::Load(m) => write!(f, "cannot load plugin: {m}"),
            PluginError::Call(m) => write!(f, "plugin failed: {m}"),
        }
    }
}

impl std::error::Error for PluginError {}
