use playspace::Playspace;

#[test]
fn set_reset_var() {
    const ABSENT: &str = "SOME_ABSENT_ENVVAR";
    const PRESENT: &str = "SOME_PRESENT_ENVVAR";
    const TRANSIENT: &str = "SOME_TRANSIENT_ENVVAR";

    std::env::remove_var(ABSENT);
    std::env::set_var(PRESENT, "present_value_before");
    std::env::set_var(TRANSIENT, "transient_value_before");

    {
        let _space = Playspace::new().expect("Failed to create space");

        std::env::set_var(ABSENT, "absent_value");
        std::env::set_var(PRESENT, "present_value_during");
        std::env::remove_var(TRANSIENT);
    }

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
