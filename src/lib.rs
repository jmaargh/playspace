#![cfg_attr(docsrs, feature(doc_cfg))]
//  SPDX-License-Identifier: MIT OR Apache-2.0
//  Licensed under either MIT Apache 2.0 licenses (attached), at your option.

//! Simple pseudo-sandbox for your convenience.
//!
//! Use these for your tests that need to set/forget files and environment
//! variables. Maybe you'll come up with more creative uses too, you're clever
//! people. It's a convenience library with no hard guarantees.
//!
//! # Aims
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
//! }).expect("Failed to create or exit playspace");
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
//! already in one with either [block][Playspace::scoped], [wait the async task][Playspace::scoped_async],
//! or [error][Playspace::try_scoped].
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
    fmt::Display,
    fs::File,
    mem::ManuallyDrop,
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
/// Preferred usage is "scoped" with a closure (similar to [`thread::spawn`][spawn]),
/// but it can also be used as an RAII-guard if necessary (similar to [`MutexGuard`][MutexGuard],
/// in which case it should be exited with [`exit()`][Playspace::exit]).
///
/// # Scoped with a closure
///
/// The program is in the Playspace only during the closure and the Playspace is
/// cleanly exited and any errors reported.
///
/// ```rust
/// # use playspace::Playspace;
/// # let path = std::path::Path::new("___playspace_test_file___.txt").to_owned();
/// assert!(!path.exists());
///
/// let path2 = path.clone();
/// let result = Playspace::scoped(move |space| {
///     println!("Now in directory: {}", space.directory().display());
///
///     space.write_file(&path2, "file contents").unwrap();
///     assert_eq!(std::fs::read_to_string(&path2).unwrap(), "file contents");
/// });
///
/// // ... handle any errors ...
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
/// # As an RAII-guard
///
/// While the `scoped*` methods should be preferred most of the time in both
/// async and non-async code, it is also possible to construct the `Playspace`
/// object directly. The program is considered "in the Playspace" from when the
/// `Playspace` is constructed until it is dropped.
///
/// It is strongly advised that you manually destroy the `Playspace` with
/// [`exit()`][Playspace::exit] so that any errors exiting the Playspace can be
/// reported. The `Drop` implementation will exit the Playspace, but any errors
/// doing so will be silently swallowed.
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
/// if let Err(exit_error) = space.exit() {
///     // ... handle errors ...
/// }
///
/// // Now we're back outside
/// assert!(!path.exists());
///
/// assert!(std::env::var("__PLAYSPACE_ENVVAR").is_err());
/// assert!(std::env::var("CARGO_MANIFEST_DIR").is_ok());
/// ```
///
/// [MutexGuard]: std::sync::MutexGuard
/// [spawn]: std::thread::spawn
pub struct Playspace {
    // N.B. field order matters! See `exit_internal`
    saved_environment: HashMap<OsString, OsString>,
    saved_current_dir: Option<PathBuf>,
    directory: ManuallyDrop<TempDir>,
    lock: ManuallyDrop<Lock>,
}

assert_impl_all!(Playspace: Send);

impl Playspace {
    /// Preferred way to use a `Playspace` in non-async code.
    ///
    /// Takes a closure, which accepts a `&mut Playspace`. Enters a new
    /// playspace, executes the closure, and exits the Playspace cleanly.
    /// Returns whatever the closure returns. The semantics of Playspace
    /// construction are the same as [`new`][Playspace::new].
    ///
    /// In async code, use [`scoped_async`][Playspace::scoped_async].
    ///
    /// # Blocks
    ///
    /// Blocks until the current process is not in a Playspace. May deadlock
    /// if called from a thread holding a `Playspace`.
    ///
    /// # Errors
    ///
    /// Returns [`SpaceError::StdIo`] if there were any system IO errors
    /// entering the Playspace, or [`SpaceError::ExitError`] for errors when
    /// exiting the Playspace.
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
        let out = f(&mut space);
        space.exit()?;

        Ok(out)
    }

    /// A scoped Playspace that doesn't block if already in one.
    ///
    /// Behaves exactly like [`scoped`][Playspace::scoped], but never blocks and
    /// already being in a Playspace is an error.
    ///
    /// In async code, use [`try_scoped_async`][Playspace::try_scoped_async].
    ///
    /// # Errors
    ///
    /// Returns [`SpaceError::AlreadyInSpace`] if already in a Playspace,
    /// [`SpaceError::StdIo`] if there were any system IO errors
    /// entering the Playspace, or [`SpaceError::ExitError`] for errors when
    /// exiting the Playspace.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use playspace::Playspace; use playspace::SpaceError::AlreadyInSpace;
    /// match Playspace::try_scoped(|space| {
    ///     space.write_file("some_file.txt", "file contents");
    ///     std::fs::read_to_string("some_file.txt").unwrap()
    /// }) {
    ///     Err(AlreadyInSpace) => { /* already in a playspace */ },
    ///     Err(_) => { /* another error */ },
    ///     Ok(file_contents) => { /* success */ },
    /// }
    /// ```
    pub fn try_scoped<R, F>(f: F) -> Result<R, SpaceError>
    where
        F: FnOnce(&mut Self) -> R,
    {
        let mut space = Self::try_new()?;
        let out = f(&mut space);
        space.exit()?;

        Ok(out)
    }

    /// Convenience combination of [`scoped`][Playspace::scoped] with implicit
    /// [`set_envs`][Playspace::set_envs].
    ///
    /// In async code, use [`scoped_with_envs_async`][Playspace::scoped_with_envs_async].
    #[allow(clippy::missing_errors_doc)]
    pub fn scoped_with_envs<I, K, V, R, F>(vars: I, f: F) -> Result<R, SpaceError>
    where
        I: IntoIterator<Item = (K, Option<V>)>,
        K: AsRef<OsStr>,
        V: AsRef<OsStr>,
        F: FnOnce(&mut Self) -> R,
    {
        let mut space = Self::with_envs(vars)?;
        let out = f(&mut space);
        space.exit()?;

        Ok(out)
    }

    /// Create a `Playspace` for use as an RAII-guard. Prefer
    /// [`scoped`][Playspace::scoped] where possible.
    ///
    /// You should destroy a Playspace created this way manually by calling
    /// [`exit`][Playspace::exit] to be able to handle errors during exiting.
    ///
    /// # Blocks
    ///
    /// Blocks until the current process is not in a Playspace. May deadlock
    /// if called from a thread holding a `Playspace`.
    ///
    /// # Errors
    ///
    /// Returns [`SpaceError::StdIo`] if there were any system IO errors
    /// entering the Playspace.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use playspace::Playspace;
    /// let space = Playspace::new().unwrap();
    /// // let space2 = Playspace::new();  // <-- This would deadlock
    /// let space2 = Playspace::try_new(); // <-- This will be an error, but not deadlock
    /// // Cleanly exit and handle any errors
    /// let exit_result = space.exit();
    /// ```
    pub fn new() -> Result<Self, SpaceError> {
        Ok(Self::from_lock(blocking_lock())?)
    }

    /// Convenience combination of [`new`][Playspace::new] followed by
    /// [`set_envs`][Playspace::set_envs]. Prefer [`scoped_with_envs`][Playspace::scoped_with_envs]
    /// where possible.
    ///
    /// In async code, use [`with_envs_async`][Playspace::with_envs_async].
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
    /// in a Playspace. Prefer [`try_scoped`][Playspace::try_scoped] or
    /// [`try_scoped_async`][Playspace::try_scoped] where possible.
    ///
    /// # Errors
    ///
    /// Returns [`SpaceError::AlreadyInSpace`] if already in a Playspace,
    /// [`SpaceError::StdIo`] if there were any system IO errors
    /// entering the Playspace, or [`SpaceError::ExitError`] for errors when
    /// exiting the Playspace.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use playspace::Playspace;
    /// let space = Playspace::try_new().unwrap();
    /// // let space2 = Playspace::new();  // <-- This would deadlock
    /// let space2 = Playspace::try_new(); // <-- This will be an error, but not deadlock
    /// // Cleanly exit and handle any errors
    /// let exit_result = space.exit();
    /// ```
    pub fn try_new() -> Result<Self, SpaceError> {
        let lock = try_lock().ok_or(SpaceError::AlreadyInSpace)?;
        Ok(Self::from_lock(lock)?)
    }

    fn from_lock(lock: Lock) -> Result<Self, std::io::Error> {
        // Lock has been taken, good.
        // Then save the environment and dir, since they're infallibe
        let saved_environment = std::env::vars_os().collect();
        let saved_current_dir = std::env::current_dir().ok();
        // This is safe to fail, no cleanup
        let directory = tempdir()?;

        // This is safe to fail, no cleanup required
        std::env::set_current_dir(directory.path())?;

        Ok(Self {
            lock: ManuallyDrop::new(lock),
            directory: ManuallyDrop::new(directory),
            saved_environment,
            saved_current_dir,
        })
    }

    /// Returns path to the directory root of the Playspace.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use playspace::Playspace;
    /// Playspace::scoped(|space| {
    ///     let spaced = space.directory();
    ///     let canonical = spaced.canonicalize().unwrap();
    ///     let temp_canonical = std::env::temp_dir()
    ///         .canonicalize()
    ///         .unwrap();
    ///     assert!(canonical.starts_with(temp_canonical));
    /// }).unwrap();
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
    /// Playspace::scoped(|space| {
    ///     space.set_envs([
    ///         ("PRESENT", Some("present_value")),
    ///         ("ABSENT", None),
    ///     ]);
    /// }).unwrap();
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
    /// Playspace::scoped(|space| {
    ///     space.write_file("some_file.txt", "some file contents").unwrap();
    /// }).unwrap();
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
    /// Playspace::scoped(|space| {
    ///     let file = space.create_file("some_file.txt").unwrap();
    /// }).unwrap();
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
    /// Playspace::scoped(|space| {
    ///     space.create_dir_all("some/non/existent/dirs").unwrap();
    /// }).unwrap();
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

    /// Leave the Playspace cleanly, reporting any errors doing so. Preferred
    /// explicit destructor over simply allowing `drop()` to be called.
    ///
    /// # Errors
    ///
    /// Returns any errors in either returning to the previous working directory
    /// or removing the temporary Playspace directory. Always attempts both
    /// operations and will report both errors if both fail.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use playspace::Playspace;
    /// {
    ///     let space = Playspace::new().unwrap();
    ///
    ///     // ... use the Playspace ...
    ///
    ///     // If this is omitted, then any errors exiting the Playspace would
    ///     // be silently ignored in `drop()`.
    ///     if let Err(error) = space.exit() {
    ///         // handle the error
    ///     }
    /// }
    /// ```
    pub fn exit(mut self) -> Result<(), ExitError> {
        let result = unsafe { self.exit_internal() };

        // At this point, no fields own heap memory or has been manually
        // dropped, so we can prevent `drop` from being called again
        std::mem::forget(self);

        result
    }

    unsafe fn exit_internal(&mut self) -> Result<(), ExitError> {
        // Infallible, do this first
        self.restore_environment();
        drop(std::mem::take(&mut self.saved_environment));

        let saved_current_dir = self.saved_current_dir.take();
        let working_dir_result = Self::restore_directory(saved_current_dir);

        let temp_dir_result = ManuallyDrop::take(&mut self.directory).close();

        // This must be done last
        drop(ManuallyDrop::take(&mut self.lock));

        match working_dir_result {
            Ok(()) => match temp_dir_result {
                Ok(()) => Ok(()),
                Err(temp) => Err(ExitError::TempDirRemoveFailed { source: temp }),
            },
            Err(working) => Err(ExitError::WorkingDirChangeFailed {
                source: working,
                temp_dir: temp_dir_result.err(),
            }),
        }
    }

    fn restore_directory(saved_current_dir: Option<PathBuf>) -> Result<(), std::io::Error> {
        if let Some(working_dir) = saved_current_dir {
            std::env::set_current_dir(working_dir)
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                "no previous working directory",
            ))
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
    /// Preferred way to use a `Playspace` in async code. Async version of
    /// [`scoped`][Playspace::scoped].
    ///
    /// The "closure" should be of the form `|space| { async move { /* code */ }.boxed() }`
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
    /// Returns [`SpaceError::StdIo`] if there were any system IO errors
    /// entering the Playspace, or [`SpaceError::ExitError`] for errors when
    /// exiting the Playspace.
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
        let out = f(&mut space).await;
        space.exit()?;

        Ok(out)
    }

    /// An async-scoped Playspace that doesn't wait if already in one. Async
    /// version of [`try_scoped`][Playspace::try_scoped].
    ///
    /// Behaves exactly like [`scoped_async`][Playspace::scoped_async], but
    /// never waits and already being in a Playspace is an error.
    ///
    /// # Errors
    ///
    /// Returns [`SpaceError::AlreadyInSpace`] if already in a Playspace,
    /// [`SpaceError::StdIo`] if there were any system IO errors
    /// entering the Playspace, or [`SpaceError::ExitError`] for errors when
    /// exiting the Playspace.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use playspace::Playspace; use playspace::SpaceError::AlreadyInSpace; use futures::FutureExt;
    /// # async {
    /// match Playspace::try_scoped_async(|space| {
    ///     async {
    ///         space.write_file("some_file.txt", "file contents");
    ///         std::fs::read_to_string("some_file.txt").unwrap()
    ///     }.boxed()
    /// }).await {
    ///     Err(AlreadyInSpace) => { /* already in a playspace */ },
    ///     Err(_) => { /* another error */ },
    ///     Ok(file_contents) => { /* success */ },
    /// }
    /// # };
    /// ```
    pub async fn try_scoped_async<R, F>(f: F) -> Result<R, SpaceError>
    where
        F: for<'a> FnOnce(&'a mut Self) -> Pin<Box<dyn Future<Output = R> + 'a>>,
    {
        let mut space = Self::try_new()?;
        let out = f(&mut space).await;
        space.exit()?;

        Ok(out)
    }

    /// Convenience combination of [`scoped_async`][Playspace::scoped_async]
    /// with implicit [`set_envs`][Playspace::set_envs].
    #[allow(clippy::missing_errors_doc)]
    pub async fn scoped_with_envs_async<I, K, V, R, F>(vars: I, f: F) -> Result<R, SpaceError>
    where
        I: IntoIterator<Item = (K, Option<V>)>,
        K: AsRef<OsStr>,
        V: AsRef<OsStr>,
        F: for<'a> FnOnce(&'a mut Self) -> Pin<Box<dyn Future<Output = R> + 'a>>,
    {
        let mut space = Self::with_envs_async(vars).await?;
        let out = f(&mut space).await;
        space.exit()?;

        Ok(out)
    }

    /// Async version of [`new`][Playspace::new]. Prefer
    /// [`scoped_async`][Playspace::scoped_async] where possible.
    ///
    /// # Waits
    ///
    /// Waits until the current process is not in a Playspace. May livelock
    /// if called from a task holding a `Playspace`.
    ///
    /// # Errors
    ///
    /// Returns [`SpaceError::StdIo`] if there were any system IO errors
    /// entering the Playspace.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use playspace::Playspace;
    /// # async {
    /// let space = Playspace::new_async().await.unwrap();
    /// // let space2 = Playspace::new().await;  // <-- This would livelock
    /// let space2 = Playspace::try_new();       // <-- This will be an error, but not livelock
    /// // Cleanly exit and handle any errors
    /// let exit_result = space.exit();
    /// # };
    /// ```
    pub async fn new_async() -> Result<Self, SpaceError> {
        Ok(Self::from_lock(MUTEX.lock().await)?)
    }

    /// Convenience combination of [`new_async`][Playspace::new_async] followed
    /// by [`set_envs`][Playspace::set_envs]. Prefer [`scoped_with_envs_async`][Playspace::scoped_with_envs_async]
    /// where possible.
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
        let _result = unsafe { self.exit_internal() };
    }
}

/// General error
#[derive(Debug, thiserror::Error)]
pub enum SpaceError {
    /// Attempted to create a (Async)Playspace while already in a (Async)Playspace.
    /// Creating either flavour while any other space exists is an error.
    #[error("already in a Playspace")]
    AlreadyInSpace,
    #[error("error exiting Playspace")]
    ExitError(#[from] ExitError),
    /// A bubbled-up error from [`std::io`] functions.
    #[error(transparent)]
    StdIo(#[from] std::io::Error),
}

/// Error writing to filesystem in Playspace
#[derive(Debug, thiserror::Error)]
pub enum WriteError {
    /// Attempted to write to a directory outside of the (Async)Playspace.
    /// The inner value is the path that was attempted to write to.
    #[error("attempt to write outside Playspace ({0})")]
    OutsidePlayspace(PathBuf),
    /// A bubbled-up error from [`std::io`] functions.
    #[error(transparent)]
    StdIo(#[from] std::io::Error),
}

#[derive(Debug)]
pub enum ExitError {
    WorkingDirChangeFailed {
        source: std::io::Error,
        temp_dir: Option<std::io::Error>,
    },
    TempDirRemoveFailed {
        source: std::io::Error,
    },
}

impl Display for ExitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::WorkingDirChangeFailed { temp_dir, .. } => match temp_dir {
                None => write!(f, "could not change working directory"),
                Some(temp) => write!(f, "could not change working directory and also encoutered an error removing temporary directory ({})", temp)
            },
            Self::TempDirRemoveFailed { .. } => write!(f, "could not remove temporary directory"),
        }
    }
}

impl std::error::Error for ExitError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(match self {
            Self::WorkingDirChangeFailed { source, .. } | Self::TempDirRemoveFailed { source } => {
                source
            }
        })
    }
}
