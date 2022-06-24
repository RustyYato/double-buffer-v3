//! an implementation of double buffers which is usable even in no_std contexts

#![no_std]
#![forbid(
    clippy::undocumented_unsafe_blocks,
    unsafe_op_in_unsafe_fn,
    clippy::missing_safety_doc
)]
#![deny(clippy::missing_docs_in_private_items)]

#[cfg(feature = "alloc")]
#[cfg(not(feature = "std"))]
extern crate alloc as std;
#[cfg(feature = "std")]
extern crate std;

pub mod ptrs;
pub mod raw;
pub mod strategy;
pub mod wait;

pub mod interface;

pub mod delayed;
#[cfg(feature = "alloc")]
pub mod op;
#[cfg(feature = "alloc")]
pub mod op_log;

#[doc(hidden)]
pub mod macros {
    pub use core;

    #[cold]
    pub fn static_writer_failed() -> ! {
        panic!("Tried to construct a static writer multiple times")
    }

    #[cold]
    pub fn assert_send_sync<T: Send + Sync>() {}
}

///
#[macro_export]
macro_rules! static_writer {
    (static $name:ident: $shared_ty:ty = $shared:expr) => {{
        // no need to require send and sync because only one writer will be able to
        // access this shared state, and that has the correct send and sync bounds
        static mut SHARED: $shared_ty = $shared;
        static FLAG: $crate::macros::core::sync::atomic::AtomicBool =
            $crate::macros::core::sync::atomic::AtomicBool::new(true);

        if FLAG
            .compare_exchange(
                true,
                false,
                $crate::macros::core::sync::atomic::Ordering::Relaxed,
                $crate::macros::core::sync::atomic::Ordering::Relaxed,
            )
            .is_err()
        {
            $crate::macros::static_writer_failed()
        }

        // SAFETY: we ensure that we're the only one to access SHARED by guarding access to FLAG
        // ONLY the first call to `static_writer` will be able to get here, so we have unqiue access
        let shared: &mut $crate::Shared<_, _> = unsafe { &mut SHARED };

        $crate::raw::Writer::new(shared)
    }};
}

///
#[macro_export]
macro_rules! try_static_writer {
    (static $name:ident: $shared_ty:ty = $shared:expr) => {{
        // no need to require send and sync because only one writer will be able to
        // access this shared state, and that has the correct send and sync bounds
        static mut SHARED: $shared_ty = $shared;
        static FLAG: $crate::macros::core::sync::atomic::AtomicBool =
            $crate::macros::core::sync::atomic::AtomicBool::new(true);

        if FLAG
            .compare_exchange(
                true,
                false,
                $crate::macros::core::sync::atomic::Ordering::Relaxed,
                $crate::macros::core::sync::atomic::Ordering::Relaxed,
            )
            .is_err()
        {
            None
        } else {
            // SAFETY: we ensure that we're the only one to access SHARED by guarding access to FLAG
            // ONLY the first call to `static_writer` will be able to get here, so we have unqiue access
            let shared: &mut $crate::raw::Shared<_, _> = unsafe { &mut SHARED };

            Some($crate::raw::Writer::new(shared))
        }
    }};
}

#[doc(hidden)]
#[test]
fn test_static_writer() {
    let count = 2;
    let waiter = std::sync::Arc::new(std::sync::Barrier::new(count));
    let writer = || try_static_writer!(static A: raw::SyncShared<[i32; 1280 * 720]> = raw::Shared::from_buffers([0; 1280 * 720], [0; 1280 * 720]));

    #[allow(clippy::needless_collect)]
    let handles = (0..count)
        .map(|_| {
            let waiter = waiter.clone();
            std::thread::spawn(move || {
                waiter.wait();
                writer()
            })
        })
        .collect::<std::vec::Vec<_>>();

    assert!(
        handles
            .into_iter()
            .filter_map(|handle| handle.join().unwrap())
            .count()
            == 1
    );
}
