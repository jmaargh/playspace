use playspace::Playspace;

#[test]
fn new_temporary() {
    let original = std::env::current_dir().expect("Invalid starting dir");
    assert!(original.exists());

    {
        let space = Playspace::default();

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

fn starting_invalid() {}
