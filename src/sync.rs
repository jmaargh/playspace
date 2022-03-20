use std::{ffi::OsStr, fs::File, path::Path};

use static_assertions::assert_impl_all;

use crate::{
    internal::Internal,
    mutex::{blocking_lock, try_lock, Lock},
    SpaceError, WriteError,
};

/// Default Playspace. Not recommended to use across `await` boundaries (in
/// `async` code use [`AsyncPlayspace`][crate::AsyncPlayspace] instead).
///
/// You can either use as an RAII-guard (similar to [`MutexGuard`][MutexGuard]), or scoped
/// with a closure (similar to [`thread::spawn`][spawn]).
///
/// # As an RAII-guard
///
/// The program is considered "in the playspace" from when the `Playspace` is
/// constructed until it is dropped.
///
/// ```rust
/// # use playspace::Playspace;
/// # let path = std::path::Path::new("___playspace_test_file___.txt");
/// // Start outside of the playspace
/// assert!(std::env::var("__PLAYSPACE_ENVVAR").is_err());
/// assert!(std::env::var("CARGO_MANIFEST_DIR").is_ok());
///
/// let space = Playspace::try_with_envs([
///     ("__PLAYSPACE_ENVVAR", Some("some value")),
///     ("CARGO_MANIFEST_DIR", None),
/// ]).expect("Probably already in a playspace");
///
/// // Now we're inside
/// println!("Now in directory: {}", space.directory().display());
///
/// assert_eq!(std::env::var("__PLAYSPACE_ENVVAR").unwrap(), "some value");
/// assert!(std::env::var("CARGO_MANIFEST_DIR").is_err());
///
/// drop(space);
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
/// # use playspace::Playspace;
/// # let path = std::path::Path::new("___playspace_test_file___.txt").to_owned();
/// assert!(!path.exists());
///
/// let path2 = path.clone();
/// let space = Playspace::scoped(move |space| {
///     println!("Now in directory: {}", space.directory().display());
///     
///     space.write_file(&path2, "file contents");
///     assert_eq!(std::fs::read_to_string(&path2).unwrap(), "file contents");
/// }).unwrap();
///
/// assert!(!path.exists());
/// ```
///
/// [MutexGuard]: std::sync::MutexGuard
/// [spawn]: std::thread::spawn
#[cfg_attr(docsrs, doc(cfg(feature = "sync")))]
pub struct Playspace {
    _lock: Lock, // NB. for drop order this MUST appear first
    internal: Internal,
}

assert_impl_all!(Playspace: Send);

impl Playspace {
    /// Create a Playspace available _only_ scoped within the given closure.
    ///
    /// Takes a closure, which accepts a `&mut Playspace`. Returns whatever the
    /// closure returns. The semantics of Playspace construction are the same
    /// as [`new`][Playspace::new].
    ///
    /// # Blocks
    ///
    /// Blocks until the current process is not in an Playspace. May deadlock
    /// if called from a thread holding a `Playspace`.
    ///
    /// # Errors
    ///
    /// Returns any system errors in creating or changing the current directory.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use playspace::Playspace;
    /// let output = Playspace::scoped(|space| {
    ///     space.write_file("some_file.txt", "file contents");
    ///     std::fs::read_to_string("some_file.txt").unwrap()
    /// }).unwrap();
    /// ```
    pub fn scoped<R, F>(f: F) -> Result<R, SpaceError>
    where
        F: FnOnce(&mut Self) -> R,
    {
        let mut space = Self::new()?;

        Ok(f(&mut space))
    }

    /// Create a Playspace available _only_ scoped within the given closure,
    /// and don't care about errors.
    ///
    /// Equivalent to [`Playspace::scoped(...).expect(...)`][Playspace::scoped].
    pub fn expect_scoped<R, F>(f: F) -> R
    where
        F: FnOnce(&mut Self) -> R,
    {
        let mut space = Self::new().expect("Failed to create playspace");

        f(&mut space)
    }

    /// Create a Playspace for use as an RAII-guard.
    ///
    /// # Blocks
    ///
    /// Blocks until the current process is not in an Playspace. May deadlock
    /// if called from a thread holding a `Playspace`.
    ///
    /// # Errors
    ///
    /// Returns any system errors in creating or changing the current directory.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use playspace::Playspace;
    /// let space = Playspace::new().unwrap();
    /// // let space2 = Playspace::new();  // <-- This would deadlock
    /// let space2 = Playspace::try_new(); // <-- This will be an error, but not deadlock
    /// ```
    pub fn new() -> Result<Self, SpaceError> {
        Ok(Self::from_lock(blocking_lock())?)
    }

    /// Convenience combination of [`new`][Playspace::new] followed by
    /// [`set_envs`][Playspace::set_envs].
    #[allow(clippy::missing_errors_doc)]
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

    /// Create a Playspace for use as an RAII-guard, do not block if already in
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
    /// # use playspace::Playspace;
    /// let space = Playspace::try_new().unwrap();
    /// // let space2 = Playspace::new();  // <-- This would deadlock
    /// let space2 = Playspace::try_new(); // <-- This will be an error, but not deadlock
    /// ```
    pub fn try_new() -> Result<Self, SpaceError> {
        let lock = try_lock().ok_or(SpaceError::AlreadyInSpace)?;
        Ok(Self::from_lock(lock)?)
    }

    /// Convenience combination of [`try_new`][Playspace::new] followed by
    /// [`set_envs`][Playspace::set_envs].
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
    /// # use playspace::Playspace;
    /// let space = Playspace::new().unwrap();
    /// let spaced = space.directory();
    /// let canonical = spaced.canonicalize().unwrap();
    /// let temp_canonical = std::env::temp_dir()
    ///     .canonicalize()
    ///     .unwrap();
    /// assert!(canonical.starts_with(temp_canonical));
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
    /// # use playspace::Playspace;
    /// let space = Playspace::new().unwrap();
    /// space.set_envs([
    ///     ("PRESENT", Some("present_value")),
    ///     ("ABSENT", None),
    /// ]);
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
    /// # use playspace::Playspace;
    /// let space = Playspace::new().unwrap();
    /// space.write_file("some_file.txt", "some file contents").unwrap();
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
    /// # use playspace::Playspace;
    /// let space = Playspace::new().unwrap();
    /// let file = space.create_file("some_file.txt").unwrap();
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
    /// # use playspace::Playspace;
    /// let space = Playspace::new().unwrap();
    /// space.create_dir_all("some/non/existent/dirs").unwrap();
    /// ```
    pub fn create_dir_all(&self, path: impl AsRef<Path>) -> Result<(), WriteError> {
        let path = self.internal.playspace_path(path)?;
        self.internal.create_dir_all(path)
    }
}
