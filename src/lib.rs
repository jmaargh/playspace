use std::path::PathBuf;

#[cfg(feature = "async")]
mod asynk;
mod internal;
#[cfg(feature = "sync")]
mod sync;

#[cfg(feature = "async")]
pub use asynk::AsyncPlayspace;
#[cfg(feature = "sync")]
pub use sync::Playspace;

#[derive(Debug, thiserror::Error)]
pub enum SpaceError {
    #[error(transparent)]
    StdIo(#[from] std::io::Error),
    #[error("Already in a Playspace")]
    AlreadyInSpace,
}

#[derive(Debug, thiserror::Error)]
pub enum WriteError {
    #[error(transparent)]
    StdIo(#[from] std::io::Error),
    #[error("Attempt to write outside Playspace: {0}")]
    OutsidePlayspace(PathBuf),
}
