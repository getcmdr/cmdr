//! macOS: the thread's QoS class, through `pthread_set_qos_class_self_np`.

use super::QosClass;

pub(crate) fn apply(class: QosClass) {
    let qos = match class {
        QosClass::Utility => libc::qos_class_t::QOS_CLASS_UTILITY,
    };
    // SAFETY: `pthread_set_qos_class_self_np` acts only on the calling thread (which is
    // live — we are running on it) and takes a valid `qos_class_t` plus a relative
    // priority offset. `qos` is one of the enum's own variants, and `0` is the documented
    // default offset, so the arguments are always in range; the call has no other
    // preconditions and cannot invalidate any Rust-side state. We log rather than unwrap
    // the return code so a hypothetical failure never aborts a worker mid-scan.
    let rc = unsafe { libc::pthread_set_qos_class_self_np(qos, 0) };
    if rc != 0 {
        log::debug!(
            target: "thread_qos",
            "pthread_set_qos_class_self_np(class={class:?}) returned {rc}"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reads back the calling thread's QoS class (as its raw `u32`) so the setter can be
    /// verified against what the OS actually recorded, not just the return code.
    /// `qos_class_t` is `#[repr(u32)]` and doesn't implement `PartialEq`, so compare values.
    fn current_qos_class_raw() -> u32 {
        let mut class = libc::qos_class_t::QOS_CLASS_UNSPECIFIED;
        // SAFETY: `pthread_get_qos_class_np` reads the given thread's QoS into `class`.
        // `pthread_self()` is always valid on the calling thread; `class` is a live,
        // writable local; the priority out-param is null, which the API accepts.
        let rc = unsafe { libc::pthread_get_qos_class_np(libc::pthread_self(), &mut class, std::ptr::null_mut()) };
        assert_eq!(rc, 0, "pthread_get_qos_class_np failed with {rc}");
        class as u32
    }

    /// The FFI must move the current thread to the requested class. Calls the arm directly
    /// (the public wrapper is a no-op in test builds), on its own spawned thread so the
    /// harness thread's QoS isn't mutated for other tests.
    #[test]
    fn sets_utility_class_on_current_thread() {
        let handle = std::thread::spawn(|| {
            apply(QosClass::Utility);
            current_qos_class_raw()
        });
        let observed = handle.join().expect("qos test thread panicked");
        assert_eq!(
            observed,
            libc::qos_class_t::QOS_CLASS_UTILITY as u32,
            "thread QoS should be UTILITY after apply(Utility)"
        );
    }
}
