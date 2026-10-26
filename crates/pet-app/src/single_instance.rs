//! Single-instance guard (named mutex) shared by the pet process.

/// Name of the per-session mutex.
pub const MUTEX_NAME: &str = r"Local\BandoriPetLite-SingleInstance";

/// Result of trying to become the primary instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Instance {
    /// This process now owns the app.
    Primary,
    /// Another instance is already running.
    AlreadyRunning,
}

/// Tries to acquire the single-instance mutex. The guard must be kept alive for
/// the lifetime of the process.
#[cfg(windows)]
pub fn acquire() -> (Instance, Option<MutexGuard>) {
    use windows::core::HSTRING;
    use windows::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS};
    use windows::Win32::System::Threading::CreateMutexW;

    let name = HSTRING::from(MUTEX_NAME);
    // SAFETY-free wrapper: CreateMutexW returns a HANDLE we own for the process
    // lifetime; the guard closes it on drop.
    let handle = match unsafe { CreateMutexW(None, false, &name) } {
        Ok(handle) => handle,
        Err(_) => return (Instance::Primary, None),
    };
    let already = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
    if already {
        (Instance::AlreadyRunning, Some(MutexGuard { handle }))
    } else {
        (Instance::Primary, Some(MutexGuard { handle }))
    }
}

#[cfg(not(windows))]
pub fn acquire() -> (Instance, Option<MutexGuard>) {
    (Instance::Primary, None)
}

/// Keeps the named mutex alive.
pub struct MutexGuard {
    #[cfg(windows)]
    handle: windows::Win32::Foundation::HANDLE,
}

#[cfg(windows)]
impl Drop for MutexGuard {
    fn drop(&mut self) {
        use windows::Win32::Foundation::CloseHandle;
        let _ = unsafe { CloseHandle(self.handle) };
    }
}
