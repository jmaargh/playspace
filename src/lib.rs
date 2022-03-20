#![cfg_attr(docsrs, feature(doc_cfg))]
//  SPDX-License-Identifier: MIT OR Apache-2.0
//  Licensed under either MIT Apache 2.0 licenses (attached), at your option.

//! Simple pseudo-sandbox for your convenience.
//!
//! Use these for your tests that need to set/forget files and environment
//! variables. Maybe you'll come up with more creative uses too, you're clever
//! people. It's a convenience library with no hard guarantees.
//!
//! # Scope
//!
//! The Playspace pseudo-sandboxes **do** provide:
//! - A new, empty, temporary working directory and return to your previous one when done
//! - Clean up for any files you create in that directory while in the Playspace
//! - Checkpoint and restore all environment variables on entering/leaving the Playspace
//! - Some basic protection against accidentally entering more than one Playspace
//! - Runtime-independent async support
//!
//! Playspaces **do not** currently provide:
//! - Actual sandboxing of any meaningful kind
//! - Any limits on being able to "break out" of the sandbox
//! - Hard guarantees on abusing multiple Playspaces at a time
//!
//! # Example
//!
//! ```rust
//! # {
//! # use playspace::Playspace;
//! Playspace::scoped(|space| {
//!     space.set_envs([
//!         ("APP_SPECIFIC_OPTION", Some("some-value")), // Set a variable
//!         ("CARGO_MANIFEST_DIR", None), // Unset another
//!     ]);
//!     space.write_file(
//!         "app-config.toml",
//!         r#"
//!         [table]
//!         option1 = 1
//!         option2 = false
//!         "#
//!     ).expect("Failed to write config file");
//!
//!     // Run some command that needs these resources...
//!
//! }).expect("Failed to create playspace");
//!
//! // Now your environment is back where we started
//! # }
//! ```
//!
//! # Async
//!
//! If you use [`Playspace`] in async code, you should enable the `async`
//! feature. Without it, holding a `Playspace` across an `.await` may cause
//! issues because of an internal mutex that needs to be async-aware.
//!
//! The `async` feature is runtime-independent and tested against [tokio](https://tokio.rs/) and
//! [async-std](https://async.rs/).
//!
//! ```toml
//! playspace = { version = "*", features = ["async"] }
//! ```
//!
//! # Details
//!
//! An application is considered "in" a Playspace when a [`Playspace`] object
//! exists. Depending on how they are created, trying to enter a Playspace when
//! already in one with either [block][Playspace::new], [wait the async task][Playspace::async_new],
//! or [error][Playspace::try_new].
//!
//! When used in tests, this conveniently stops tests that create and destroy
//! files from interacting, even when run concurrently.
//!
//! When entering a Playspace, a new temporary directory is created and the
//! working directory is moved to that directory. A snapshot of all environment
//! variables is internally saved.
//!
//! When leaving the Playspace, the former working directory is returned to and
//! the Playspace directory is removed. All environment variables are reset to
//! their state before entering the Playspace.
//!
//! Convenience functions like [`write_file`][Playspace::write_file] and
//! [`set_envs`][Playspace::set_envs] are provided, but they are nothing more
//! than convenience. They will prevent you from accidentally writing to outside
//! of the Playspace, but there's nothing stopping you from using other methods
//! (e.g. [`std::fs::write`] or [`std::env::set_var`]) to do whatever you want.
//!

use std::{
    collections::HashMap,
    ffi::{OsStr, OsString},
    fs::File,
    path::{Path, PathBuf},
};
#[cfg(feature = "async")]
use std::{future::Future, pin::Pin};

mod mutex;

#[cfg(feature = "async")]
use mutex::MUTEX;
use mutex::{blocking_lock, try_lock, Lock};
use static_assertions::assert_impl_all;
use tempfile::{tempdir, TempDir};

/// Playspace, while the object exists you are "in" the playspace.
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
/// let space = Playspace::with_envs([
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
/// Playspace::scoped(move |space| {
///     println!("Now in directory: {}", space.directory().display());
///
///     space.write_file(&path2, "file contents").unwrap();
///     assert_eq!(std::fs::read_to_string(&path2).unwrap(), "file contents");
/// }).unwrap();
///
/// assert!(!path.exists());
/// ```
///
/// # Async
///
/// Since a `Playspace` holds a mutex, it is important to use the feature
/// `async` when using in async code, otherwise the whole thread can be
/// unnecessarily blocked.
///
/// The `async` feature also provides some more "async-friendly" methods.
/// However, the struct is safe to use in async code so long as the feature is
/// enabled, regardless of which methods are used.
///
/// ```rust
/// # use playspace::Playspace;
/// # let path = std::path::Path::new("___playspace_test_file___.txt").to_owned();
/// use futures::FutureExt; // Needed for `FutureExt::boxed`
/// assert!(!path.exists());
///
/// let path2 = path.clone();
/// async {
///     Playspace::scoped_async(move |space| {
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
pub struct Playspace {
    _lock: Lock, // NB. for drop order this MUST appear first
    saved_current_dir: Option<PathBuf>,
    saved_environment: HashMap<OsString, OsString>,
    directory: TempDir,
}

assert_impl_all!(Playspace: Send);

impl Playspace {
    /// Create a `Playspace` available _only_ scoped within the given closure.
    ///
    /// Takes a closure, which accepts a `&mut Playspace`. Returns whatever the
    /// closure returns. The semantics of Playspace construction are the same
    /// as [`new`][Playspace::new].
    ///
    /// # Blocks
    ///
    /// Blocks until the current process is not in a Playspace. May deadlock
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

    /// Convenience combination of [`scoped`][Playspace::scoped] with implicit
    /// [`set_envs`][Playspace::set_envs].
    #[allow(clippy::missing_errors_doc)]
    pub fn scoped_with_envs<I, K, V, R, F>(vars: I, f: F) -> Result<R, SpaceError>
    where
        I: IntoIterator<Item = (K, Option<V>)>,
        K: AsRef<OsStr>,
        V: AsRef<OsStr>,
        F: FnOnce(&mut Self) -> R,
    {
        let mut space = Self::with_envs(vars)?;

        Ok(f(&mut space))
    }

    /// Create a `Playspace` for use as an RAII-guard.
    ///
    /// # Blocks
    ///
    /// Blocks until the current process is not in a Playspace. May deadlock
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

    /// Create a `Playspace` for use as an RAII-guard, do not block if already
    /// in a Playspace.
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
        self.directory.path()
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
        for (key, value) in vars {
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            };
        }
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
        let path = self.playspace_path(path)?;
        Ok(std::fs::write(path, contents)?)
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
        let path = self.playspace_path(path)?;
        Ok(std::fs::File::create(path)?)
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
        let path = self.playspace_path(path)?;
        Ok(std::fs::create_dir_all(path)?)
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

#[cfg(feature = "async")]
#[cfg_attr(docsrs, doc(cfg(feature = "async")))]
impl Playspace {
    /// Async version of [`scoped`][Playspace::scoped].
    ///
    /// The "closure" should be of the form `|space| { async move {}.boxed() }`
    /// -- where `boxed()` is from [`futures::FutureExt`](https://docs.rs/futures/latest/futures/future/trait.FutureExt.html) --
    /// because of [this](https://stackoverflow.com/a/70539457) syntax issue.
    ///
    /// # Waits
    ///
    /// Waits until the current process is not in a Playspace. May livelock
    /// if called from a task holding a `Playspace`.
    ///
    /// # Errors
    ///
    /// Returns any system errors in creating or changing the current directory.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use playspace::Playspace; use futures::FutureExt;
    /// # async {
    /// let output = Playspace::scoped_async(|space| {
    ///     async {
    ///         space.write_file("some_file.txt", "file contents");
    ///         std::fs::read_to_string("some_file.txt").unwrap()
    ///     }.boxed()
    /// }).await.unwrap();
    /// # };
    /// ```
    pub async fn scoped_async<R, F>(f: F) -> Result<R, SpaceError>
    where
        F: for<'a> FnOnce(&'a mut Self) -> Pin<Box<dyn Future<Output = R> + 'a>>,
    {
        let mut space = Self::new_async().await?;
        Ok(f(&mut space).await)
    }

    /// Convenience combination of [`scoped_async`][Playspace::scoped_async]
    /// with implicit [`set_envs`][Playspace::set_envs].
    #[allow(clippy::missing_errors_doc)]
    pub async fn scoped_with_envs_async<I, K, V, R, F>(vars: I, f: F) -> Result<R, SpaceError>
    where
        I: IntoIterator<Item = (K, Option<V>)>,
        K: AsRef<OsStr>,
        V: AsRef<OsStr>,
        F: FnOnce(&mut Self) -> R,
    {
        let mut space = Self::with_envs_async(vars).await?;
        Ok(f(&mut space))
    }

    /// Async version of [`new`][Playspace::new].
    ///
    /// # Waits
    ///
    /// Waits until the current process is not in a Playspace. May livelock
    /// if called from a task holding a `Playspace`.
    ///
    /// # Errors
    ///
    /// Returns any system errors in creating or changing the current directory.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use playspace::Playspace;
    /// # async {
    /// let space = Playspace::new_async().await.unwrap();
    /// // let space2 = Playspace::new().await;  // <-- This would livelock
    /// let space2 = Playspace::try_new();       // <-- This will be an error, but not livelock
    /// # };
    /// ```
    pub async fn new_async() -> Result<Self, SpaceError> {
        Ok(Self::from_lock(MUTEX.lock().await)?)
    }

    /// Convenience combination of [`new_async`][Playspace::new_async] followed
    /// by [`set_envs`][Playspace::set_envs].
    #[allow(clippy::missing_errors_doc)]
    pub async fn with_envs_async<I, K, V>(vars: I) -> Result<Self, SpaceError>
    where
        I: IntoIterator<Item = (K, Option<V>)>,
        K: AsRef<OsStr>,
        V: AsRef<OsStr>,
    {
        let out = Self::new()?;
        out.set_envs(vars);
        Ok(out)
    }
}

impl Drop for Playspace {
    fn drop(&mut self) {
        self.restore_directory();
        self.restore_environment();
    }
}

/// General error
#[derive(Debug, thiserror::Error)]
pub enum SpaceError {
    /// Attempted to create a (Async)Playspace while already in a (Async)Playspace.
    /// Creating either flavour while any other space exists is an error.
    #[error("Already in a Playspace")]
    AlreadyInSpace,
    /// A bubbled-up error from [`std::io`] functions.
    #[error(transparent)]
    StdIo(#[from] std::io::Error),
}

/// Error writing to filesystem in Playspace
#[derive(Debug, thiserror::Error)]
pub enum WriteError {
    /// Attempted to write to a directory outside of the (Async)Playspace.
    /// The inner value is the path that was attempted to write to.
    #[error("Attempt to write outside Playspace: {0}")]
    OutsidePlayspace(PathBuf),
    /// A bubbled-up error from [`std::io`] functions.
    #[error(transparent)]
    StdIo(#[from] std::io::Error),
}
