use parking_lot::{const_mutex, Mutex, MutexGuard};

static MUTEX: Mutex<()> = const_mutex(());

pub struct Playspace {
    _lock: MutexGuard<'static, ()>,
}

impl Playspace {
    #[must_use]
    pub fn new() -> Self {
        Self {
            _lock: MUTEX.lock(),
        }
    }
}

impl Default for Playspace {
    fn default() -> Self {
        Self::new()
    }
}
