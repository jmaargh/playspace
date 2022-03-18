use std::{
    collections::HashMap,
    ffi::{OsStr, OsString},
    fs::File,
    path::{Path, PathBuf},
};

use tempfile::{tempdir, TempDir};

use crate::WriteError;

pub(crate) struct Internal {
    directory: TempDir,
    saved_current_dir: Option<PathBuf>,
    saved_environment: HashMap<OsString, OsString>,
}

impl Internal {
    pub(crate) fn new() -> Result<Self, std::io::Error> {
        let out = Self {
            directory: tempdir()?,
            saved_current_dir: std::env::current_dir().ok(),
            saved_environment: std::env::vars_os().collect(),
        };

        std::env::set_current_dir(out.directory())?;

        Ok(out)
    }

    #[allow(clippy::must_use_candidate)]
    pub(crate) fn directory(&self) -> &Path {
        self.directory.path()
    }

    #[allow(clippy::unused_self)]
    pub(crate) fn set_envs<I, K, V>(&self, vars: I)
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

    pub(crate) fn write_file<P, C>(&self, path: P, contents: C) -> Result<(), WriteError>
    where
        P: AsRef<Path>,
        C: AsRef<[u8]>,
    {
        let path = self.playspace_path(path)?;
        Ok(std::fs::write(path, contents)?)
    }

    pub(crate) fn create_file(&self, path: impl AsRef<Path>) -> Result<File, WriteError> {
        let path = self.playspace_path(path)?;
        Ok(std::fs::File::create(path)?)
    }

    pub(crate) fn create_dir_all(&self, path: impl AsRef<Path>) -> Result<(), WriteError> {
        let path = self.playspace_path(path)?;
        Ok(std::fs::create_dir_all(path)?)
    }

    pub fn playspace_path(&self, path: impl AsRef<Path>) -> Result<PathBuf, WriteError> {
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

impl Drop for Internal {
    fn drop(&mut self) {
        self.restore_directory();
        self.restore_environment();
    }
}
