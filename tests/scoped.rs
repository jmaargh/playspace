use std::{cell::RefCell, path::PathBuf, rc::Rc};

use serial_test::serial;

use playspace::Playspace;

const ABSENT: &str = "SOME_ABSENT_ENVVAR";
const PRESENT: &str = "SOME_PRESENT_ENVVAR";
const TRANSIENT: &str = "SOME_TRANSIENT_ENVVAR";

fn set_vars_before() {
    std::env::remove_var(ABSENT);
    std::env::set_var(PRESENT, "present_value_before");
    std::env::set_var(TRANSIENT, "transient_value_before");
}

fn assert_envs_inside() {
    assert_eq!(std::env::var(ABSENT), Ok("absent_value".to_owned()));
    assert_eq!(
        std::env::var(PRESENT),
        Ok("present_value_during".to_owned())
    );
    assert_eq!(
        std::env::var(TRANSIENT),
        Err(std::env::VarError::NotPresent)
    );
}

fn assert_envs_outside() {
    assert_eq!(std::env::var(ABSENT), Err(std::env::VarError::NotPresent));
    assert_eq!(
        std::env::var(PRESENT),
        Ok("present_value_before".to_owned())
    );
    assert_eq!(
        std::env::var(TRANSIENT),
        Ok("transient_value_before".to_owned())
    );
}

#[test]
#[serial]
fn no_nested() {
    Playspace::scoped(|_space| {
        #[allow(clippy::match_wild_err_arm)]
        match Playspace::try_new() {
            Err(playspace::SpaceError::AlreadyInSpace) => (),
            Err(_) => panic!("Wrong error"),
            Ok(_) => panic!("Should not be possibel"),
        }
    })
    .expect("Failed to create playspace");
}

#[test]
#[serial]
fn files_and_envs() {
    set_vars_before();
    assert_envs_outside();

    let path = Rc::new(RefCell::new(PathBuf::from("some_file.txt")));
    let path_during = path.clone();

    Playspace::scoped(move |space| {
        space.set_envs([
            (ABSENT, Some("absent_value")),
            (PRESENT, Some("present_value_during")),
        ]);

        space
            .write_file(&*path_during.borrow(), "some file contents")
            .unwrap();
        path_during.replace(space.directory().join("some_file.txt"));

        assert_eq!(
            std::fs::read_to_string(&*path_during.borrow()).unwrap(),
            "some file contents"
        );

        space.set_envs([(TRANSIENT, Option::<&str>::None)]);

        assert_envs_inside();
    })
    .unwrap();

    assert!(!path.borrow().exists());

    assert_envs_outside();
}
