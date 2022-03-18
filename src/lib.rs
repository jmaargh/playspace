use std::path::{Path, PathBuf};

use parking_lot::{const_mutex, Mutex, MutexGuard};
use tempfile::{tempdir, TempDir};

static MUTEX: Mutex<()> = const_mutex(());

pub struct Playspace {
    _lock: MutexGuard<'static, ()>,
    directory: TempDir,
    saved_current_dir: PathBuf,
}

impl Playspace {
    #[must_use]
    pub fn new() -> Self {
        let current = std::env::current_dir().unwrap();
        println!("{current:?}");

        let out = Self {
            _lock: MUTEX.lock(),
            directory: tempdir().unwrap(),
            saved_current_dir: std::env::current_dir().unwrap(),
        };

        std::env::set_current_dir(out.directory()).unwrap();

        out
    }

    pub fn directory(&self) -> &Path {
        self.directory.path()
    }
}

impl Drop for Playspace {
    fn drop(&mut self) {
        std::env::set_current_dir(&self.saved_current_dir).unwrap();
    }
}

impl Default for Playspace {
    fn default() -> Self {
        Self::new()
    }
}
