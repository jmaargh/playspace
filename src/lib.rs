use std::{
    collections::HashMap,
    ffi::{OsStr, OsString},
    fs::File,
    path::{Path, PathBuf},
};

use parking_lot::const_mutex;
use static_assertions::assert_impl_all;
use tempfile::{tempdir, TempDir};

static MUTEX: Mutex = const_mutex(LockType());

pub struct Playspace {
    _lock: Lock,
    directory: TempDir,
    saved_current_dir: Option<PathBuf>,
    saved_environment: HashMap<OsString, OsString>,
}

assert_impl_all!(Playspace: Send);

impl Playspace {
    pub fn new() -> Result<Self, SpaceError> {
        Ok(Self::from_lock(MUTEX.lock())?)
    }

    pub fn with_envs<I, K, V>(vars: I) -> Result<Self, SpaceError>
    where
        I: IntoIterator<Item = (K, Option<V>)>,
        K: AsRef<OsStr>,
        V: AsRef<OsStr>,
    {
        let out = Self::new()?;
        out.env_vars(vars);
        Ok(out)
    }

    pub fn try_new() -> Result<Self, SpaceError> {
        let lock = MUTEX.try_lock().ok_or(SpaceError::AlreadyInSpace)?;
        Ok(Self::from_lock(lock)?)
    }

    pub fn try_with_envs<I, K, V>(vars: I) -> Result<Self, SpaceError>
    where
        I: IntoIterator<Item = (K, Option<V>)>,
        K: AsRef<OsStr>,
        V: AsRef<OsStr>,
    {
        let out = Self::try_new()?;
        out.env_vars(vars);
        Ok(out)
    }

    fn from_lock(lock: Lock) -> Result<Self, std::io::Error> {
        let out = Self {
            _lock: lock,
            directory: tempdir()?,
            saved_current_dir: std::env::current_dir().ok(),
            saved_environment: std::env::vars_os().collect(),
        };

        std::env::set_current_dir(out.directory())?;

        Ok(out)
    }

    #[allow(clippy::must_use_candidate)]
    pub fn directory(&self) -> &Path {
        self.directory.path()
    }

    #[allow(clippy::unused_self)]
    pub fn env_vars<I, K, V>(&self, vars: I)
    where
        I: IntoIterator<Item = (K, Option<V>)>,
        K: AsRef<OsStr>,
        V: AsRef<OsStr>,
    {
        for (key, value) in vars {
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            };
        }
    }

    pub fn write_file<P, C>(&self, path: P, contents: C) -> Result<(), WriteError>
    where
        P: AsRef<Path>,
        C: AsRef<[u8]>,
    {
        let canonical = self.playspace_path(path)?;
        Ok(std::fs::write(canonical, contents)?)
    }

    pub fn create_file(&self, path: impl AsRef<Path>) -> Result<File, WriteError> {
        let canonical = self.playspace_path(path)?;
        Ok(std::fs::File::create(canonical)?)
    }

    fn playspace_path(&self, path: impl AsRef<Path>) -> Result<PathBuf, WriteError> {
        if path.as_ref().is_relative() {
            // Simple case, just assume it was meant to be relative to the of the space
            Ok(self.directory().join(path))
        } else {
            // Ensure that the absolute path given is actually in the playspace
            for ancestor in path.as_ref().ancestors() {
                if ancestor.exists() {
                    // Found a parent
                    let canonical_ancestor = ancestor.canonicalize()?;
                    if !canonical_ancestor.starts_with(self.directory().canonicalize()?) {
                        // Not in the playspace
                        return Err(WriteError::OutsidePlayspace(path.as_ref().into()));
                    }
                    return Ok(path.as_ref().into());
                }
            }

            // Couldn't find a parent in the playspace
            Err(WriteError::OutsidePlayspace(path.as_ref().into()))
        }
    }

    fn restore_directory(&self) {
        if let Some(working_dir) = &self.saved_current_dir {
            let _result = std::env::set_current_dir(working_dir);
        }
    }

    fn restore_environment(&mut self) {
        for (variable, _value) in std::env::vars_os() {
            match self.saved_environment.remove(&variable) {
                Some(saved_value) => std::env::set_var(&variable, saved_value),
                None => std::env::remove_var(&variable),
            }
        }
        for (removed_variable, value) in self.saved_environment.drain() {
            std::env::set_var(removed_variable, value);
        }
    }
}

impl Drop for Playspace {
    fn drop(&mut self) {
        self.restore_directory();
        self.restore_environment();
    }
}

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

/// Type used to guarantee that locked are only creatable from this crate
struct LockType();
type Mutex = parking_lot::Mutex<LockType>;
type Lock = parking_lot::MutexGuard<'static, LockType>;
