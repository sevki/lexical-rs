use std::cell::RefCell;
use std::fmt;

#[derive(Debug)]
pub enum PluginError {
    /// The bytes are not a valid component or do not implement the expected interface.
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

/// Most failures kept per plugin; older ones are dropped so a plugin that fails on every
/// command cannot make the host's memory grow without limit.
const MAX_ERRORS: usize = 64;

pub(crate) fn record(errors: &RefCell<Vec<PluginError>>, error: PluginError) {
    let mut errors = errors.borrow_mut();
    if errors.len() >= MAX_ERRORS {
        errors.remove(0);
    }
    errors.push(error);
}
