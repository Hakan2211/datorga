//! OS helpers around a recording: free disk space and keeping the machine awake.

use std::marker::PhantomData;
use std::path::Path;

/// Bytes available to this user on the volume holding `path` (or its nearest existing
/// ancestor). `None` where the platform call is unavailable or fails.
pub fn free_space(path: &Path) -> Option<u64> {
    let existing = path.ancestors().find(|p| p.exists())?;
    imp::free_space(existing)
}

/// Keeps the system from idle sleep while it lives (the display may still turn off). The
/// Windows flag belongs to the calling thread, so the guard must be dropped on the thread that
/// created it: it is deliberately `!Send`.
pub struct KeepAwake {
    active: bool,
    _not_send: PhantomData<*const ()>,
}

impl KeepAwake {
    pub fn new() -> Self {
        Self { active: imp::keep_awake(true), _not_send: PhantomData }
    }

    /// Whether the OS accepted the request.
    pub fn active(&self) -> bool {
        self.active
    }
}

impl Default for KeepAwake {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for KeepAwake {
    fn drop(&mut self) {
        if self.active {
            imp::keep_awake(false);
        }
    }
}

#[cfg(windows)]
mod imp {
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;

    const ES_CONTINUOUS: u32 = 0x8000_0000;
    const ES_SYSTEM_REQUIRED: u32 = 0x0000_0001;

    #[link(name = "kernel32")]
    extern "system" {
        fn GetDiskFreeSpaceExW(dir: *const u16, free_to_caller: *mut u64, total: *mut u64, total_free: *mut u64) -> i32;
        fn SetThreadExecutionState(flags: u32) -> u32;
    }

    pub fn free_space(path: &Path) -> Option<u64> {
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
        let (mut free, mut total, mut total_free) = (0u64, 0u64, 0u64);
        // SAFETY: `wide` is NUL-terminated and outlives the call; the out pointers are valid.
        let ok = unsafe { GetDiskFreeSpaceExW(wide.as_ptr(), &mut free, &mut total, &mut total_free) };
        (ok != 0).then_some(free)
    }

    pub fn keep_awake(on: bool) -> bool {
        let flags = if on { ES_CONTINUOUS | ES_SYSTEM_REQUIRED } else { ES_CONTINUOUS };
        // SAFETY: plain flag call; returns the previous state, 0 on failure.
        unsafe { SetThreadExecutionState(flags) != 0 }
    }
}

#[cfg(not(windows))]
mod imp {
    use std::path::Path;

    pub fn free_space(_path: &Path) -> Option<u64> {
        None
    }

    pub fn keep_awake(_on: bool) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn free_space_of_missing_dir_uses_existing_ancestor() {
        let dir = std::env::temp_dir();
        let direct = free_space(&dir);
        let nested = free_space(&dir.join("does-not-exist").join("deeper"));
        if cfg!(windows) {
            assert!(direct.is_some_and(|b| b > 0));
            assert!(nested.is_some());
        }
    }

    #[test]
    fn keep_awake_is_accepted_and_released() {
        let guard = KeepAwake::new();
        if cfg!(windows) {
            assert!(guard.active());
        }
        #[cfg(windows)]
        assert!(system_requires_awake(), "system-wide execution state must include ES_SYSTEM_REQUIRED");
        drop(guard);
    }

    /// Reads the system-wide execution state (no admin rights needed, unlike `powercfg /requests`).
    #[cfg(windows)]
    fn system_requires_awake() -> bool {
        #[link(name = "powrprof")]
        extern "system" {
            fn CallNtPowerInformation(level: i32, input: *const u8, input_len: u32, output: *mut u32, output_len: u32) -> i32;
        }
        const SYSTEM_EXECUTION_STATE: i32 = 16;
        let mut state = 0u32;
        // SAFETY: output points to a u32 of the stated size; no input buffer.
        let status = unsafe { CallNtPowerInformation(SYSTEM_EXECUTION_STATE, std::ptr::null(), 0, &mut state, 4) };
        status == 0 && state & 0x1 != 0
    }
}
