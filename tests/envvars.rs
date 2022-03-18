#![cfg(feature = "sync")]

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
fn set_reset_var() {
    set_vars_before();
    assert_envs_outside();

    {
        let _space = Playspace::new().expect("Failed to create space");

        std::env::set_var(ABSENT, "absent_value");
        std::env::set_var(PRESENT, "present_value_during");
        std::env::remove_var(TRANSIENT);

        assert_envs_inside();
    }

    assert_envs_outside();
}

#[test]
#[serial]
fn multi_vars_syntax() {
    set_vars_before();
    assert_envs_outside();

    {
        let space = Playspace::new().expect("Failed to create space");
        space.set_envs([
            (ABSENT, Some("absent_value")),
            (PRESENT, Some("present_value_during")),
            (TRANSIENT, None),
        ]);

        assert_envs_inside();
    }

    assert_envs_outside();
}

#[test]
#[serial]
fn with_envs() {
    set_vars_before();
    assert_envs_outside();

    {
        let _space = Playspace::with_envs([
            (ABSENT, Some("absent_value")),
            (PRESENT, Some("present_value_during")),
            (TRANSIENT, None),
        ])
        .expect("Failed to create space");

        assert_envs_inside();
    }

    assert_envs_outside();
}

#[test]
#[serial]
fn try_with_envs() {
    set_vars_before();
    assert_envs_outside();

    {
        let _space = Playspace::try_with_envs([
            (ABSENT, Some("absent_value")),
            (PRESENT, Some("present_value_during")),
            (TRANSIENT, None),
        ])
        .expect("Failed to create space");

        assert_envs_inside();
    }

    assert_envs_outside();
}
