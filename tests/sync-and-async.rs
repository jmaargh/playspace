#![cfg(all(feature = "async", feature = "sync"))]

use std::sync::{
    atomic::{AtomicU32, Ordering},
    Arc,
};

use lazy_static::lazy_static;
use playspace::{AsyncPlayspace, Playspace};

lazy_static! {
    static ref SERIAL: async_std::sync::Mutex<()> = async_std::sync::Mutex::new(());
}

#[async_std::test]
async fn async_blocks_sync() {
    let _serial = SERIAL.lock().await;

    let counter1 = Arc::new(AtomicU32::new(0));
    let counter2 = counter1.clone();

    std::env::remove_var("SOME_VAR");
    let async_space = AsyncPlayspace::with_envs([("SOME_VAR", Some("some value"))])
        .await
        .unwrap();
    assert_eq!(std::env::var("SOME_VAR").unwrap(), "some value");

    let handle = std::thread::spawn(move || {
        assert_eq!(counter2.load(Ordering::Acquire), 0);
        std::thread::yield_now();

        let _sync_space = Playspace::new().unwrap(); // We're testing that this blocks ...

        assert_eq!(counter2.load(Ordering::Acquire), 2);

        counter2.fetch_add(1, Ordering::Release);
        assert_eq!(counter2.load(Ordering::Acquire), 3);

        counter2.fetch_add(1, Ordering::Release);
        assert_eq!(counter2.load(Ordering::Acquire), 4);
    });

    // Give the other thread ample time to do some work if it's not blocked
    std::thread::yield_now();
    std::thread::sleep(std::time::Duration::from_millis(200));

    counter1.fetch_add(1, Ordering::Release);
    std::thread::yield_now();
    assert_eq!(counter1.load(Ordering::Acquire), 1);
    std::thread::yield_now();

    counter1.fetch_add(1, Ordering::Release);
    std::thread::yield_now();
    assert_eq!(counter1.load(Ordering::Acquire), 2);
    std::thread::yield_now();

    drop(async_space); // ... until this

    handle.join().expect("Thread panic");

    assert_eq!(counter1.load(Ordering::Acquire), 4);
}

#[async_std::test]
async fn sync_blocks_async() {
    let _serial = SERIAL.lock().await;

    let counter1 = Arc::new(AtomicU32::new(0));
    let counter2 = counter1.clone();

    let sync_space = Playspace::new().unwrap();

    async_std::task::spawn(async move {
        assert_eq!(counter2.load(Ordering::Acquire), 0);
        std::thread::yield_now();

        let _async_space = AsyncPlayspace::new().await.unwrap(); // We're testing that this blocks ...

        assert_eq!(counter2.load(Ordering::Acquire), 2);

        counter2.fetch_add(1, Ordering::Release);
        assert_eq!(counter2.load(Ordering::Acquire), 3);

        counter2.fetch_add(1, Ordering::Release);
        assert_eq!(counter2.load(Ordering::Acquire), 4);
    });

    // Give the other thread ample time to do some work if it's not blocked
    std::thread::yield_now();
    std::thread::sleep(std::time::Duration::from_millis(200));

    counter1.fetch_add(1, Ordering::Release);
    std::thread::yield_now();
    assert_eq!(counter1.load(Ordering::Acquire), 1);
    std::thread::yield_now();

    counter1.fetch_add(1, Ordering::Release);
    std::thread::yield_now();
    assert_eq!(counter1.load(Ordering::Acquire), 2);
    std::thread::yield_now();

    drop(sync_space); // ... until this

    assert_eq!(counter1.load(Ordering::Acquire), 4);
}
