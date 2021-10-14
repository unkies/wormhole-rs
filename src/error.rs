use std::io;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum WormholeError {
    /// Happens when an error happens during io operations.
    #[error("io operation failed")]
    Io(#[from] io::Error),

    /// Happens when underlying syscall returns an error.
    #[error("syscall error")]
    Sys(#[from] nix::errno::Errno),

    /// Happens when an error happens during serialization or deserialization.
    #[error("serialization/deserialization failed")]
    Serialization(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, WormholeError>;
