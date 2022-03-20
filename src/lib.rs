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
//!
//! Playspaces **do not** currently provide:
//! - Actual sandboxing of any meaningful kind
//! - Any limits on being able to "break out" of the sandbox
//! - Hard guarantees on abusing multiple Playspaces at a time
//!
//! # Example
//!
//! ```rust
//! # #[cfg(feature = "sync")]
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
//! # Flavours
//!
//! The public API exists in two flavours: [`Playspace`] and [`AsyncPlayspace`].
//! They are equivalent functionaly and appropriate for codebases without and
//! with async code respectively.
//!
//! [`AsyncPlayspace`] is runtime-independent and tested against [tokio](https://tokio.rs/) and
//! [async-std](https://async.rs/).
//!
//! Features:
//! - `sync` (is `default`): provides `Playspace`
//! - `async`: provides `AsyncPlayspace`
//!
//! If you only need `AsyncPlayspace` depend on
//! ```toml
//! playspace = { version = "*", default-features = false, features = ["async"] }
//! ```
//!
//! # Details
//!
//! An application is considered "in" a Playspace when a [`Playspace`] or
//! [`AsyncPlayspace`] object exists. Depending on how they are created,
//! trying to enter a Playspace when already in one with either [block][Playspace::new]
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

use std::path::PathBuf;

#[cfg(feature = "async")]
mod asynk;
mod internal;
mod mutex;
#[cfg(feature = "sync")]
mod sync;

/// `async`-friendly Playspace
#[cfg(feature = "async")]
pub use asynk::AsyncPlayspace;
/// Default Playspace, does not play nicely with `async` code
#[cfg(feature = "sync")]
pub use sync::Playspace;

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
