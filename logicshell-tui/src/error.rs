use thiserror::Error;

#[derive(Debug, Error)]
pub enum TuiError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("core error: {0}")]
    Core(#[from] logicshell_core::LogicShellError),
}

pub type Result<T> = std::result::Result<T, TuiError>;
