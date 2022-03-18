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

        assert_eq!(spaced, space.directory());

        std::fs::create_dir("a_subdir").expect("Failed to create subdirectory");
        std::env::set_current_dir("a_subdir").expect("Failed to move to subdirectory");
        assert_ne!(std::env::current_dir().unwrap(), space.directory());
    }

    let ending = std::env::current_dir().expect("Invalid final dir");
    assert_eq!(original, ending);
    assert!(ending.exists());
}

#[test]
#[serial]
fn starting_invalid() {
    let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
    let temp_path = temp_dir.path().to_owned();
    std::env::set_current_dir(&temp_path).expect("Failed to move to temp dir");
    drop(temp_dir);

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
}
