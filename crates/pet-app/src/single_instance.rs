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

#[cfg(target_os = "macos")]
pub fn acquire() -> (Instance, Option<MutexGuard>) {
    match acquire_macos() {
        Ok(result) => result,
        Err(error) => {
            tracing::error!(%error, "could not acquire single-instance lock");
            // Do not start a second config writer when the lock is inaccessible.
            (Instance::AlreadyRunning, None)
        }
    }
}

#[cfg(target_os = "macos")]
fn acquire_macos() -> std::io::Result<(Instance, Option<MutexGuard>)> {
    let home = std::env::var_os("HOME")
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "HOME is not set"))?;
    let directory = std::path::PathBuf::from(home).join("Library/Caches/BandoriPetLite");
    std::fs::create_dir_all(&directory)?;
    acquire_lock(&directory.join("instance.lock"))
}

#[cfg(target_os = "macos")]
fn acquire_lock(path: &std::path::Path) -> std::io::Result<(Instance, Option<MutexGuard>)> {
    use std::os::unix::fs::OpenOptionsExt;
    use std::os::unix::io::AsRawFd;

    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(path)?;
    // flock is released by the kernel even after a crash. Keep the file (and its
    // inode) in place when unlocking so simultaneous launches cannot bypass it.
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
        Ok((Instance::Primary, Some(MutexGuard { _file: file })))
    } else {
        let error = std::io::Error::last_os_error();
        if error.kind() == std::io::ErrorKind::WouldBlock {
            Ok((Instance::AlreadyRunning, None))
        } else {
            Err(error)
        }
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
pub fn acquire() -> (Instance, Option<MutexGuard>) {
    (Instance::Primary, None)
}

/// Keeps the named mutex alive.
pub struct MutexGuard {
    #[cfg(windows)]
    handle: windows::Win32::Foundation::HANDLE,
    #[cfg(target_os = "macos")]
    _file: std::fs::File,
}

#[cfg(windows)]
impl Drop for MutexGuard {
    fn drop(&mut self) {
        use windows::Win32::Foundation::CloseHandle;
        let _ = unsafe { CloseHandle(self.handle) };
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    #[test]
    fn only_one_writer_can_run_and_dropping_the_guard_allows_restart() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("instance.lock");
        let (instance, guard) = acquire_lock(&path).unwrap();
        assert_eq!(instance, Instance::Primary);
        assert_eq!(acquire_lock(&path).unwrap().0, Instance::AlreadyRunning);
        drop(guard);
        assert_eq!(acquire_lock(&path).unwrap().0, Instance::Primary);
    }
}
