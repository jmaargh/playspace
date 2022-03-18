use std::path::{Path, PathBuf};

use parking_lot::const_mutex;
use tempfile::{tempdir, TempDir};

static MUTEX: Mutex = const_mutex(LockType());

pub struct Playspace {
    _lock: Lock,
    directory: TempDir,
    saved_current_dir: Option<PathBuf>,
}

impl Playspace {
    pub fn new() -> Result<Self, SpaceError> {
        Ok(Self::from_lock(MUTEX.lock())?)
    }

    pub fn try_new() -> Result<Self, SpaceError> {
        let lock = MUTEX.try_lock().ok_or(SpaceError::AlreadyInSpace)?;
        Ok(Self::from_lock(lock)?)
    }

    fn from_lock(lock: Lock) -> Result<Self, std::io::Error> {
        let out = Self {
            _lock: lock,
            directory: tempdir()?,
            saved_current_dir: std::env::current_dir().ok(),
        };

        std::env::set_current_dir(out.directory())?;

        Ok(out)
    }

    #[allow(clippy::must_use_candidate)]
    pub fn directory(&self) -> &Path {
        self.directory.path()
    }
}

impl Drop for Playspace {
    fn drop(&mut self) {
        if let Some(working_dir) = &self.saved_current_dir {
            let _result = std::env::set_current_dir(working_dir);
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SpaceError {
    #[error(transparent)]
    StdIo(#[from] std::io::Error),
    #[error("Already in a Playspace")]
    AlreadyInSpace,
}

/// Type used to guarantee that locked are only creatable from this crate
struct LockType();
type Mutex = parking_lot::Mutex<LockType>;
type Lock = parking_lot::MutexGuard<'static, LockType>;
