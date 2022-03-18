use std::{ffi::OsStr, fs::File, future::Future, path::Path, pin::Pin};

use lazy_static::lazy_static;
use static_assertions::assert_impl_all;

use crate::{internal::Internal, SpaceError, WriteError};

lazy_static! {
    static ref MUTEX: Mutex = Mutex::new(LockType());
}

pub struct AsyncPlayspace {
    _lock: Lock,
    internal: Internal,
}

assert_impl_all!(AsyncPlayspace: Send);

impl AsyncPlayspace {
    // N.B. you need to `boxed()` your futures because of [this](https://stackoverflow.com/a/70539457) syntax issue
    // ```
    // Jail::with_async(|jail| {
    //     async {
    //         // Your code
    //     }.boxed()
    // });
    // ```
    pub async fn scoped<R, F>(f: F) -> Result<R, SpaceError>
    where
        F: for<'a> FnOnce(&'a mut Self) -> Pin<Box<dyn Future<Output = R> + 'a>>,
    {
        let mut space = Self::new().await?;

        Ok(f(&mut space).await)
    }

    // N.B. you need to `boxed()` your futures because of [this](https://stackoverflow.com/a/70539457) syntax issue
    // ```
    // Jail::with_async(|jail| {
    //     async {
    //         // Your code
    //     }.boxed()
    // });
    // ```
    pub async fn expect_scoped<R, F>(f: F) -> R
    where
        F: for<'a> FnOnce(&'a mut Self) -> Pin<Box<dyn Future<Output = R> + 'a>>,
    {
        let mut space = Self::new().await.expect("Failed to create playspace");

        f(&mut space).await
    }

    pub async fn new() -> Result<Self, SpaceError> {
        Ok(Self::from_lock(MUTEX.lock().await)?)
    }

    pub async fn with_envs<I, K, V>(vars: I) -> Result<Self, SpaceError>
    where
        I: IntoIterator<Item = (K, Option<V>)>,
        K: AsRef<OsStr>,
        V: AsRef<OsStr>,
    {
        let out = Self::new().await?;
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
type Mutex = async_mutex::Mutex<LockType>;
type Lock = async_mutex::MutexGuard<'static, LockType>;
