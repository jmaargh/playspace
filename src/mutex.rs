//  SPDX-License-Identifier: MIT OR Apache-2.0
//  Licensed under either MIT Apache 2.0 licenses (attached), at your option.

pub(crate) use internal::*;

/// Type used to guarantee that locked are only creatable from this crate
pub(crate) struct LockType();

#[cfg(all(not(feature = "async"), feature = "sync"))]
mod internal {
    use parking_lot::const_mutex;

    use super::LockType;

    pub(crate) static MUTEX: Mutex = const_mutex(LockType());

    pub(crate) type Mutex = parking_lot::Mutex<LockType>;
    pub(crate) type Lock = parking_lot::MutexGuard<'static, LockType>;

    #[inline]
    pub(crate) fn blocking_lock() -> Lock {
        MUTEX.lock()
    }

    #[inline]
    pub(crate) fn try_lock() -> Option<Lock> {
        MUTEX.try_lock()
    }
}

#[cfg(feature = "async")]
mod internal {
    use super::LockType;

    pub(crate) static MUTEX: Mutex = Mutex::const_new(LockType());

    pub(crate) type Mutex = tokio::sync::Mutex<LockType>;
    pub(crate) type Lock = tokio::sync::MutexGuard<'static, LockType>;

    #[cfg(feature = "sync")]
    #[inline]
    pub(crate) fn blocking_lock() -> Lock {
        MUTEX.blocking_lock()
    }

    #[cfg(feature = "sync")]
    #[inline]
    pub(crate) fn try_lock() -> Option<Lock> {
        MUTEX.try_lock().ok()
    }
}
