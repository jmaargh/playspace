use std::{ffi::OsStr, fs::File, path::Path};

use parking_lot::const_mutex;
use static_assertions::assert_impl_all;

use crate::{internal::Internal, SpaceError, WriteError};

// FIXME: should also prevent creating a sync playspace in an async one and vice versa
static MUTEX: Mutex = const_mutex(LockType());

#[cfg_attr(docsrs, doc(cfg(feature = "sync")))]
pub struct Playspace {
    _lock: Lock, // NB. for drop order this MUST appear first
    internal: Internal,
}

assert_impl_all!(Playspace: Send);

impl Playspace {
    pub fn scoped<R, F>(f: F) -> Result<R, SpaceError>
    where
        F: FnOnce(&mut Self) -> R,
    {
        let mut space = Self::new()?;

        Ok(f(&mut space))
    }

    pub fn expect_scoped<R, F>(f: F) -> R
    where
        F: FnOnce(&mut Self) -> R,
    {
        let mut space = Self::new().expect("Failed to create playspace");

        f(&mut space)
    }

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
        out.set_envs(vars);
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
        out.set_envs(vars);
        Ok(out)
    }

    fn from_lock(lock: Lock) -> Result<Self, std::io::Error> {
        Ok(Self {
            _lock: lock,
            internal: Internal::new()?,
        })
    }

    #[allow(clippy::must_use_candidate)]
    pub fn directory(&self) -> &Path {
        self.internal.directory()
    }

    #[allow(clippy::unused_self)]
    pub fn set_envs<I, K, V>(&self, vars: I)
    where
        I: IntoIterator<Item = (K, Option<V>)>,
        K: AsRef<OsStr>,
        V: AsRef<OsStr>,
    {
        self.internal.set_envs(vars);
    }

    pub fn write_file<P, C>(&self, path: P, contents: C) -> Result<(), WriteError>
    where
        P: AsRef<Path>,
        C: AsRef<[u8]>,
    {
        self.internal.write_file(path, contents)
    }

    pub fn create_file(&self, path: impl AsRef<Path>) -> Result<File, WriteError> {
        self.internal.create_file(path)
    }

    pub fn create_dir_all(&self, path: impl AsRef<Path>) -> Result<(), WriteError> {
        let path = self.internal.playspace_path(path)?;
        self.internal.create_dir_all(path)
    }
}

/// Type used to guarantee that locked are only creatable from this crate
struct LockType();
type Mutex = parking_lot::Mutex<LockType>;
type Lock = parking_lot::MutexGuard<'static, LockType>;
