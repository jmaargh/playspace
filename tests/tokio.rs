#![cfg(feature = "async")]

use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicU32, Ordering},
        Arc,
    },
};

use futures::FutureExt;
use parking_lot::Mutex;

use playspace::Playspace;

const ABSENT: &str = "SOME_ABSENT_ENVVAR";
const PRESENT: &str = "SOME_PRESENT_ENVVAR";
const TRANSIENT: &str = "SOME_TRANSIENT_ENVVAR";

static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

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

#[tokio::test]
async fn files_and_envs() {
    let _serial = SERIAL.lock().await;

    set_vars_before();
    assert_envs_outside();

    let path = Arc::new(Mutex::new(PathBuf::from("some_file.txt")));
    let path_during = path.clone();

    Playspace::scoped_async(move |space| {
        async move {
            space.set_envs([
                (ABSENT, Some("absent_value")),
                (PRESENT, Some("present_value_during")),
            ]);

            space
                .write_file(&*path_during.lock(), "some file contents")
                .unwrap();
            *path_during.lock() = space.directory().join("some_file.txt");

            assert_eq!(
                tokio::fs::read_to_string(&*path_during.lock())
                    .await
                    .unwrap(),
                "some file contents"
            );

            space.set_envs([(TRANSIENT, Option::<&str>::None)]);

            assert_envs_inside();
        }
        .boxed()
    })
    .await
    .unwrap();

    assert!(!path.lock().exists());

    assert_envs_outside();
}

#[tokio::test]
async fn wait_when_spaced() {
    let _serial = SERIAL.lock().await;

    let space1 = Playspace::new_async()
        .await
        .expect("Failed to create space");

    let counter1 = Arc::new(AtomicU32::new(0));
    let counter2 = counter1.clone();

    assert_eq!(counter1.load(Ordering::Acquire), 0);
    tokio::task::yield_now().await;

    let handle = tokio::spawn(async move {
        assert_eq!(counter2.load(Ordering::Acquire), 0);
        tokio::task::yield_now().await;

        let _space2 = Playspace::new_async()
            .await
            .expect("Failed to create second space"); // We're testing that this blocks ...

        assert_eq!(counter2.load(Ordering::Acquire), 2);

        counter2.fetch_add(1, Ordering::Release);
        assert_eq!(counter2.load(Ordering::Acquire), 3);

        counter2.fetch_add(1, Ordering::Release);
        assert_eq!(counter2.load(Ordering::Acquire), 4);
    });

    // Give the other thread ample time to do some work if it's not blocked
    tokio::task::yield_now().await;
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    counter1.fetch_add(1, Ordering::Release);
    tokio::task::yield_now().await;
    assert_eq!(counter1.load(Ordering::Acquire), 1);
    tokio::task::yield_now().await;

    counter1.fetch_add(1, Ordering::Release);
    tokio::task::yield_now().await;
    assert_eq!(counter1.load(Ordering::Acquire), 2);
    tokio::task::yield_now().await;

    drop(space1); // ... until this

    handle.await.expect("Thread panic");

    assert_eq!(counter1.load(Ordering::Acquire), 4);
}
