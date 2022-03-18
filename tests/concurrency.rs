use std::sync::{
    atomic::{AtomicU32, Ordering},
    Arc,
};

use serial_test::serial;

use playspace::{Playspace, SpaceError};

#[test]
#[serial]
fn wait_when_spaced() {
    let space1 = Playspace::new().expect("Failed to create space");

    let counter1 = Arc::new(AtomicU32::new(0));
    let counter2 = counter1.clone();

    assert_eq!(counter1.load(Ordering::Acquire), 0);
    std::thread::yield_now();

    let handle = std::thread::spawn(move || {
        assert_eq!(counter2.load(Ordering::Acquire), 0);
        std::thread::yield_now();

        let _space2 = Playspace::new().expect("Failed to create second space"); // We're testing that this blocks ...

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

    drop(space1); // ... until this

    handle.join().expect("Thread panic");

    assert_eq!(counter1.load(Ordering::Acquire), 4);
}

#[test]
#[serial]
fn fail_when_spaced() {
    {
        let _space1 = Playspace::try_new().expect("Failed to create space");

        if let Err(SpaceError::AlreadyInSpace) = Playspace::try_new() {
        } else {
            panic!("Shouldn't be able to create an inner-space")
        }
    }

    assert!(Playspace::try_new().is_ok());
}

#[test]
#[serial]
fn fail_when_spaced_thread() {
    {
        let _space1 = Playspace::try_new().expect("Failed to create space");

        let handle = std::thread::spawn(|| {
            if let Err(SpaceError::AlreadyInSpace) = Playspace::try_new() {
            } else {
                panic!("Shouldn't be able to create an inner-space")
            }
        });

        handle.join().expect("Thread panic");
    }

    assert!(Playspace::try_new().is_ok());
}
