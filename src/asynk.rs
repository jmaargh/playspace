use std::{ffi::OsStr, fs::File, future::Future, path::Path, pin::Pin};

use static_assertions::assert_impl_all;

use crate::{
    internal::Internal,
    mutex::{Lock, MUTEX},
    SpaceError, WriteError,
};

/// `async`-friendly Playspace.
///
/// You can either use as an RAII-guard (similar to [`MutexGuard`][MutexGuard]),
/// or scoped with a closure (similar to [`thread::spawn`][spawn]).
///
/// # As an RAII-guard
///
/// The program is considered "in the playspace" from when the `AsyncPlayspace`
/// is constructed until it is dropped.
///
/// ```rust
/// # use playspace::AsyncPlayspace;
/// # let path = std::path::Path::new("___playspace_test_file___.txt");
/// // Start outside of the playspace
/// assert!(std::env::var("__PLAYSPACE_ENVVAR").is_err());
/// assert!(std::env::var("CARGO_MANIFEST_DIR").is_ok());
///
/// async {
///     let space = AsyncPlayspace::with_envs([
///         ("__PLAYSPACE_ENVVAR", Some("some value")),
///         ("CARGO_MANIFEST_DIR", None),
///     ]).await.expect("Probably already in a playspace");
///
///     // Now we're inside
///     println!("Now in directory: {}", space.directory().display());
///
///     assert_eq!(std::env::var("__PLAYSPACE_ENVVAR").unwrap(), "some value");
///     assert!(std::env::var("CARGO_MANIFEST_DIR").is_err());
/// };
/// // Now we're back outside
///
/// assert!(!path.exists());
///
/// assert!(std::env::var("__PLAYSPACE_ENVVAR").is_err());
/// assert!(std::env::var("CARGO_MANIFEST_DIR").is_ok());
/// ```
///
/// # Scoped with a closure
///
/// The program is in the Playspace only during the closure.
///
/// ```rust
/// # use playspace::AsyncPlayspace;
/// # let path = std::path::Path::new("___playspace_test_file___.txt").to_owned();
/// use futures::FutureExt; // Needed for `FutureExt::boxed`
/// assert!(!path.exists());
///
/// let path2 = path.clone();
/// async {
///     AsyncPlayspace::scoped(move |space| {
///         async move {
///             println!("Now in directory: {}", space.directory().display());
///
///             space.write_file(&path2, "file contents").unwrap();
///             assert_eq!(std::fs::read_to_string(&path2).unwrap(), "file contents");
///         }.boxed()
///     }).await.unwrap();
/// };
///
/// assert!(!path.exists());
/// ```
///
/// [MutexGuard]: std::sync::MutexGuard
/// [spawn]: std::thread::spawn
#[cfg_attr(docsrs, doc(cfg(feature = "async")))]
pub struct AsyncPlayspace {
    _lock: Lock, // NB. for drop order this MUST appear first
    internal: Internal,
}

assert_impl_all!(AsyncPlayspace: Send);

impl AsyncPlayspace {
    /// Create an `AsyncPlayspace` available _only_ scoped within the given
    /// async "closure".
    ///
    /// The "closure" should be of the form `|space| { async move {}.boxed() }`
    /// because of [this](https://stackoverflow.com/a/70539457) syntax issue.
    ///
    /// Returns whatever the closure returns. The semantics of Playspace
    /// construction are the same as [`new`][AsyncPlayspace::new].
    ///
    /// # Blocks
    ///
    /// Blocks until the current process is not in a Playspace. May deadlock
    /// if called from a task holding a `Playspace` or `AsyncPlayspace`.
    ///
    /// # Errors
    ///
    /// Returns any system errors in creating or changing the current directory.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use playspace::AsyncPlayspace; use futures::FutureExt;
    /// # async {
    /// let output = AsyncPlayspace::scoped(|space| {
    ///     async {
    ///         space.write_file("some_file.txt", "file contents");
    ///         std::fs::read_to_string("some_file.txt").unwrap()
    ///     }.boxed()
    /// }).await.unwrap();
    /// # };
    /// ```
    pub async fn scoped<R, F>(f: F) -> Result<R, SpaceError>
    where
        F: for<'a> FnOnce(&'a mut Self) -> Pin<Box<dyn Future<Output = R> + 'a>>,
    {
        let mut space = Self::new().await?;

        Ok(f(&mut space).await)
    }

    /// Create a `Playspace` available _only_ scoped within the given closure,
    /// and don't care about errors.
    ///
    /// Equivalent to [`AsyncPlayspace::scoped(...).await.expect(...)`][AsyncPlayspace::scoped].
    pub async fn expect_scoped<R, F>(f: F) -> R
    where
        F: for<'a> FnOnce(&'a mut Self) -> Pin<Box<dyn Future<Output = R> + 'a>>,
    {
        let mut space = Self::new().await.expect("Failed to create playspace");

        f(&mut space).await
    }

    /// Create an `AsyncPlayspace` for use as an RAII-guard.
    ///
    /// # Waits
    ///
    /// Waits until the current process is not in a Playspace. May livelock
    /// if called from a task holding a `Playspace` or `AsyncPlayspace`.
    ///
    /// # Errors
    ///
    /// Returns any system errors in creating or changing the current directory.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use playspace::AsyncPlayspace;
    /// # async {
    /// let space = AsyncPlayspace::new().await.unwrap();
    /// // let space2 = AsyncPlayspace::new().await; // <-- This would livelock
    /// let space2 = AsyncPlayspace::try_new();      // <-- This will be an error, but not livelock
    /// # };
    /// ```
    pub async fn new() -> Result<Self, SpaceError> {
        Ok(Self::from_lock(MUTEX.lock().await)?)
    }

    /// Convenience combination of [`new`][AsyncPlayspace::new] followed by
    /// [`set_envs`][AsyncPlayspace::set_envs].
    #[allow(clippy::missing_errors_doc)]
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

    /// Create an `AsyncPlayspace` for use as an RAII-guard, do not wait if already in
    /// a Playspace.
    ///
    /// # Errors
    ///
    /// Returns any system errors in creating or changing the current directory.
    /// Returns [`AlreadyInSpace`][crate::SpaceError::AlreadyInSpace] if
    /// process is already in a Playspace.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use playspace::AsyncPlayspace;
    /// let space = AsyncPlayspace::try_new().unwrap();
    /// // let space2 = Playspace::new().await; // <-- This would livelock
    /// let space2 = AsyncPlayspace::try_new(); // <-- This will be an error, but not livelock
    /// ```
    pub fn try_new() -> Result<Self, SpaceError> {
        let lock = MUTEX.try_lock().map_err(|_| SpaceError::AlreadyInSpace)?;
        Ok(Self::from_lock(lock)?)
    }

    /// Convenience combination of [`try_new`][AsyncPlayspace::new] followed by
    /// [`set_envs`][AsyncPlayspace::set_envs].
    #[allow(clippy::missing_errors_doc)]
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

    /// Returns path to the directory root of the Playspace.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use playspace::AsyncPlayspace;
    /// # async {
    /// let space = AsyncPlayspace::new().await.unwrap();
    /// let spaced = space.directory();
    /// let canonical = spaced.canonicalize().unwrap();
    /// let temp_canonical = std::env::temp_dir()
    ///     .canonicalize()
    ///     .unwrap();
    /// assert!(canonical.starts_with(temp_canonical));
    /// # };
    /// ```
    #[allow(clippy::must_use_candidate)]
    pub fn directory(&self) -> &Path {
        self.internal.directory()
    }

    /// Set or unset several environment variables.
    ///
    /// Pass an iterable of `(environmentvariable, value)` pairs. If the value
    /// is `None` the variable is unset, otherwise it is set to the value.
    ///
    /// Equivalent to repeated calls to `std::env::set_var` and
    /// `std::env::remove_var`.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use playspace::AsyncPlayspace;
    /// # async {
    /// let space = AsyncPlayspace::new().await.unwrap();
    /// space.set_envs([
    ///     ("PRESENT", Some("present_value")),
    ///     ("ABSENT", None),
    /// ]);
    /// # };
    /// ```
    #[allow(clippy::unused_self)]
    pub fn set_envs<I, K, V>(&self, vars: I)
    where
        I: IntoIterator<Item = (K, Option<V>)>,
        K: AsRef<OsStr>,
        V: AsRef<OsStr>,
    {
        self.internal.set_envs(vars);
    }

    /// Write a file to the Playspace.
    ///
    /// Relative paths are _always_ evaluated with respect to the Playspace
    /// root directory, even if the current directory has since changed. Whether
    /// the given path is relative or absolute, this checks that the given
    /// path is inside the Playspace.
    ///
    /// # Errors
    ///
    /// If the provided path is not in the Playspace, an error will be returned.
    /// Any stardard IO error is bubbled-up.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use playspace::AsyncPlayspace;
    /// # async {
    /// let space = AsyncPlayspace::new().await.unwrap();
    /// space.write_file("some_file.txt", "some file contents").unwrap();
    /// # };
    /// ```
    pub fn write_file<P, C>(&self, path: P, contents: C) -> Result<(), WriteError>
    where
        P: AsRef<Path>,
        C: AsRef<[u8]>,
    {
        self.internal.write_file(path, contents)
    }

    /// Create a file in the Playspace, returning the [`File`][std::fs::File]
    /// object.
    ///
    /// Relative paths are _always_ evaluated with respect to the Playspace
    /// root directory, even if the current directory has since changed. Whether
    /// the given path is relative or absolute, this checks that the given
    /// path is inside the Playspace.
    ///
    /// # Errors
    ///
    /// If the provided path is not in the Playspace, an error will be returned.
    /// Any stardard IO error is bubbled-up.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use playspace::AsyncPlayspace;
    /// # async {
    /// let space = AsyncPlayspace::new().await.unwrap();
    /// let file = space.create_file("some_file.txt").unwrap();
    /// # };
    /// ```
    pub fn create_file(&self, path: impl AsRef<Path>) -> Result<File, WriteError> {
        self.internal.create_file(path)
    }

    /// Create one or more directories in the Playspace, similar to [`std::fs::create_dir_all`].
    ///
    /// Relative paths are _always_ evaluated with respect to the Playspace
    /// root directory, even if the current directory has since changed. Whether
    /// the given path is relative or absolute, this checks that the given
    /// path is inside the Playspace.
    ///
    /// # Errors
    ///
    /// If the provided path is not in the Playspace, an error will be returned.
    /// Any stardard IO error is bubbled-up.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use playspace::AsyncPlayspace;
    /// # async {
    /// let space = AsyncPlayspace::new().await.unwrap();
    /// space.create_dir_all("some/non/existent/dirs").unwrap();
    /// # };
    /// ```
    pub fn create_dir_all(&self, path: impl AsRef<Path>) -> Result<(), WriteError> {
        let path = self.internal.playspace_path(path)?;
        self.internal.create_dir_all(path)
    }
}
