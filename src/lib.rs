#![cfg_attr(docsrs, feature(doc_cfg))]

//! Simple pseudo-sandbox for your convenience.
//!
//! Use these for your tests that need to set/forget files and environment
//! variables. Maybe you'll come up with more creative uses too, you're clever
//! people. It's a convenience library with no hard guarantees.
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
//! ```rust
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
//! ```
//!
//! The public API exists in two flavours: [`Playspace`] and [`AsyncPlayspace`].
//! They are equivalent functionaly and appropriate for non-async and async
//! codebases respectively.
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

use std::path::PathBuf;

#[cfg(feature = "async")]
mod asynk;
mod internal;
#[cfg(feature = "sync")]
mod sync;

#[cfg(feature = "async")]
pub use asynk::AsyncPlayspace;
#[cfg(feature = "sync")]
pub use sync::Playspace;

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
