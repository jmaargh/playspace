use serial_test::serial;

use playspace::Playspace;

#[test]
#[serial]
fn new_temporary() {
    let original = std::env::current_dir().expect("Invalid starting dir");
    assert!(original.exists());

    {
        let space = Playspace::new().expect("Failed to create space");

        let spaced = std::env::current_dir().expect("Invalid spaced dir");
        assert_ne!(original, spaced);
        assert!(spaced.exists());

        // MacOS seems to require that these be canonicalised otherwise they don't compare equal
        assert_eq!(
            spaced.canonicalize().unwrap(),
            space.directory().canonicalize().unwrap()
        );

        let spaced_canonical = spaced.canonicalize().expect("Failed to canonicalise");
        let temp_canonical = std::env::temp_dir()
            .canonicalize()
            .expect("Failed to canonicalise temp dir");
        assert!(spaced_canonical.starts_with(&temp_canonical));

        std::fs::create_dir("a_subdir").expect("Failed to create subdirectory");
        std::env::set_current_dir("a_subdir").expect("Failed to move to subdirectory");
        assert_ne!(std::env::current_dir().unwrap(), space.directory());
    }

    let ending = std::env::current_dir().expect("Invalid final dir");
    assert_eq!(original, ending);
    assert!(ending.exists());
}

// This test is disabled on Windows, because it's based on the premise of
// deleting the working directory from under the process, but Windows explicitly
// forbids this.
#[cfg(not(target_os = "windows"))]
#[test]
#[serial]
fn starting_invalid() {
    let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
    let temp_path = temp_dir.path().to_owned();
    std::env::set_current_dir(&temp_path).expect("Failed to move to temp dir");
    temp_dir.close().unwrap();

    // We've purposefully poisoned our CWD
    assert!(!temp_path.exists());
    assert!(std::env::current_dir().is_err());

    let space = Playspace::new().expect("Failed to create space");

    let spaced_cwd = std::env::current_dir().expect("Failed to get spaced CWD");
    assert!(spaced_cwd.exists());
    assert_ne!(temp_path, spaced_cwd);

    drop(space);

    assert!(std::env::current_dir().is_err());

    let space2 = Playspace::new().expect("Failed to create second space");
    let spaced_cwd = std::env::current_dir().expect("Failed to get spaced CWD");
    assert!(spaced_cwd.exists());
    assert_ne!(temp_path, spaced_cwd);

    drop(space2);
    assert!(std::env::current_dir().is_err());

    // Tidy up to be nice to other tests
    std::env::set_current_dir(std::env::var("CARGO_MANIFEST_DIR").unwrap()).unwrap();
}
