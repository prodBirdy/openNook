//! Checked wrappers for the few FFI patterns we own, plus canonical macOS
//! framework declarations under [`macos`].
//!
//! Helpers do not erase `unsafe` — they concentrate null/size/identity checks
//! so call sites stay thin and the invariants live in one place.

#[cfg(target_os = "macos")]
pub mod macos;

/// Signal a process group with `SIGTERM`.
///
/// Refuses `pgid <= 1` (invalid / init) and our own process group so a
/// corrupted pid file cannot tear down the app.
#[cfg(unix)]
pub fn kill_process_group(pgid: i32) {
    if pgid <= 1 {
        return;
    }
    // SAFETY: getpgrp has no preconditions.
    let ours = unsafe { libc::getpgrp() };
    if pgid == ours {
        log::warn!("refusing killpg on our own process group ({pgid})");
        return;
    }
    // SAFETY: pgid > 1 and not ours; SIGTERM is a valid signal.
    unsafe {
        let _ = libc::killpg(pgid, libc::SIGTERM);
    }
}

#[cfg(not(unix))]
pub fn kill_process_group(_pgid: i32) {}

/// Copy `len` bytes from `ptr`, or `None` if null / empty.
///
/// # Safety
/// If `ptr` is non-null and `len > 0`, it must be valid for `len` reads.
#[inline]
pub unsafe fn copy_bytes(ptr: *const u8, len: usize) -> Option<Vec<u8>> {
    if ptr.is_null() || len == 0 {
        return None;
    }
    Some(std::slice::from_raw_parts(ptr, len).to_vec())
}

/// Borrow `len` elements, or an empty slice if null / empty.
///
/// # Safety
/// If `ptr` is non-null and `len > 0`, it must be valid for `len` reads
/// for the returned lifetime.
#[inline]
pub unsafe fn slice<'a, T>(ptr: *const T, len: usize) -> &'a [T] {
    if ptr.is_null() || len == 0 {
        &[]
    } else {
        std::slice::from_raw_parts(ptr, len)
    }
}

/// Resolve a `dlsym` symbol into a function pointer of type `T`.
///
/// # Safety
/// `T` must match the C ABI of the named symbol. Handle must come from
/// `dlopen` (or be null, which yields `None`).
#[cfg(unix)]
pub unsafe fn dlsym_fn<T>(handle: *mut libc::c_void, name: &std::ffi::CStr) -> Option<T> {
    if handle.is_null() {
        return None;
    }
    debug_assert_eq!(
        std::mem::size_of::<T>(),
        std::mem::size_of::<*mut libc::c_void>(),
        "dlsym target must be pointer-sized"
    );
    let ptr = libc::dlsym(handle, name.as_ptr());
    if ptr.is_null() {
        None
    } else {
        Some(std::mem::transmute_copy(&ptr))
    }
}

/// Cast an `extern "C"` ObjC method implementation to [`objc2::runtime::Imp`].
///
/// # Safety
/// `f` must match the ObjC calling convention for the selector's type encoding.
#[cfg(target_os = "macos")]
#[inline]
pub unsafe fn as_objc_imp<F>(f: F) -> objc2::runtime::Imp {
    debug_assert_eq!(
        std::mem::size_of::<F>(),
        std::mem::size_of::<objc2::runtime::Imp>()
    );
    std::mem::transmute_copy(&f)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kill_process_group_rejects_invalid() {
        kill_process_group(0);
        kill_process_group(-1);
        kill_process_group(1);
    }

    #[test]
    fn copy_bytes_null_or_empty() {
        assert!(unsafe { copy_bytes(std::ptr::null(), 4) }.is_none());
        let b = [1u8, 2];
        assert!(unsafe { copy_bytes(b.as_ptr(), 0) }.is_none());
        assert_eq!(unsafe { copy_bytes(b.as_ptr(), 2) }, Some(vec![1, 2]));
    }

    #[test]
    fn slice_null_or_empty() {
        assert!(unsafe { slice::<u8>(std::ptr::null(), 3) }.is_empty());
        let b = [9u8, 8, 7];
        assert_eq!(unsafe { slice(b.as_ptr(), 3) }, &[9, 8, 7]);
    }
}
