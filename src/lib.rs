use std::path::{Path, PathBuf};

use parking_lot::{const_mutex, Mutex, MutexGuard};
use tempfile::{tempdir, TempDir};

static MUTEX: Mutex<()> = const_mutex(());

pub struct Playspace {
    _lock: MutexGuard<'static, ()>,
    directory: TempDir,
    saved_current_dir: Option<PathBuf>,
}

impl Playspace {
    #[must_use]
    pub fn new() -> Result<Self, SpaceError> {
        let out = Self {
            _lock: MUTEX.lock(),
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
}
