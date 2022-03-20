#![cfg(feature = "async")]

use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicU32, Ordering},
        Arc,
    },
};

use futures::FutureExt;
use lazy_static::lazy_static;
use parking_lot::Mutex;

use playspace::Playspace;

const ABSENT: &str = "SOME_ABSENT_ENVVAR";
const PRESENT: &str = "SOME_PRESENT_ENVVAR";
const TRANSIENT: &str = "SOME_TRANSIENT_ENVVAR";

lazy_static! {
    static ref SERIAL: async_std::sync::Mutex<()> = async_std::sync::Mutex::new(());
}

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

#[async_std::test]
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

            let mut path_during = path_during.lock();
            space
                .write_file(&*path_during, "some file contents")
                .unwrap();
            *path_during = space.directory().join("some_file.txt");

            assert_eq!(
                async_std::fs::read_to_string(&*path_during).await.unwrap(),
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

#[async_std::test]
async fn wait_when_spaced() {
    let _serial = SERIAL.lock().await;

    let space1 = Playspace::new_async()
        .await
        .expect("Failed to create space");

    let counter1 = Arc::new(AtomicU32::new(0));
    let counter2 = counter1.clone();

    assert_eq!(counter1.load(Ordering::Acquire), 0);
    async_std::task::yield_now().await;

    let handle = async_std::task::spawn(async move {
        assert_eq!(counter2.load(Ordering::Acquire), 0);
        async_std::task::yield_now().await;

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
    async_std::task::yield_now().await;
    async_std::task::sleep(std::time::Duration::from_millis(200)).await;

    counter1.fetch_add(1, Ordering::Release);
    async_std::task::yield_now().await;
    assert_eq!(counter1.load(Ordering::Acquire), 1);
    async_std::task::yield_now().await;

    counter1.fetch_add(1, Ordering::Release);
    async_std::task::yield_now().await;
    assert_eq!(counter1.load(Ordering::Acquire), 2);
    async_std::task::yield_now().await;

    drop(space1); // ... until this

    handle.await;

    assert_eq!(counter1.load(Ordering::Acquire), 4);
}
