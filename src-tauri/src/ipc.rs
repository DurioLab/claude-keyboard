use std::io::{self, Read, Write};
use std::time::Duration;

/// IPC socket/pipe path (platform-dependent)
#[cfg(unix)]
pub const IPC_PATH: &str = "/tmp/claude-keyboard.sock";
#[cfg(windows)]
pub const IPC_PATH: &str = r"\\.\pipe\claude-keyboard";

// ─────────────────────────────────────────────
// Unix implementation
// ─────────────────────────────────────────────
#[cfg(unix)]
mod platform {
    use super::*;
    use std::os::unix::net::{UnixListener as StdUnixListener, UnixStream as StdUnixStream};

    pub struct IpcListener {
        inner: StdUnixListener,
    }

    impl IpcListener {
        pub fn bind() -> io::Result<Self> {
            // Remove stale socket file if it exists
            let _ = std::fs::remove_file(IPC_PATH);
            let inner = StdUnixListener::bind(IPC_PATH)?;
            Ok(Self { inner })
        }

        pub fn accept(&self) -> io::Result<IpcStream> {
            let (stream, _addr) = self.inner.accept()?;
            Ok(IpcStream { inner: stream })
        }
    }

    impl Drop for IpcListener {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(IPC_PATH);
        }
    }

    pub struct IpcStream {
        inner: StdUnixStream,
    }

    impl IpcStream {
        pub fn connect() -> io::Result<Self> {
            let inner = StdUnixStream::connect(IPC_PATH)?;
            Ok(Self { inner })
        }

        pub fn set_read_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
            self.inner.set_read_timeout(timeout)
        }

        pub fn shutdown_write(&self) -> io::Result<()> {
            self.inner.shutdown(std::net::Shutdown::Write)
        }
    }

    impl Read for IpcStream {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            self.inner.read(buf)
        }
    }

    impl Write for IpcStream {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.inner.write(buf)
        }

        fn flush(&mut self) -> io::Result<()> {
            self.inner.flush()
        }
    }
}

// ─────────────────────────────────────────────
// Windows implementation
// ─────────────────────────────────────────────
#[cfg(windows)]
mod platform {
    use super::*;
    use std::ptr;
    use windows_sys::Win32::Foundation::{
        CloseHandle, GetLastError, ERROR_PIPE_CONNECTED, GENERIC_READ, GENERIC_WRITE, HANDLE,
        INVALID_HANDLE_VALUE,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, FlushFileBuffers, ReadFile, WriteFile, OPEN_EXISTING,
    };
    use windows_sys::Win32::System::Pipes::{
        ConnectNamedPipe, CreateNamedPipeW, PIPE_READMODE_BYTE, PIPE_TYPE_BYTE,
        PIPE_UNLIMITED_INSTANCES, PIPE_WAIT,
    };

    // PIPE_ACCESS_DUPLEX is not exported in windows-sys 0.59; define manually.
    const PIPE_ACCESS_DUPLEX: u32 = 0x00000003;

    const BUFFER_SIZE: u32 = 65536;

    fn wide_path() -> Vec<u16> {
        IPC_PATH.encode_utf16().chain(std::iter::once(0)).collect()
    }

    pub struct IpcListener {
        _created: bool,
    }

    impl IpcListener {
        pub fn bind() -> io::Result<Self> {
            Ok(Self { _created: true })
        }

        pub fn accept(&self) -> io::Result<IpcStream> {
            let path = wide_path();
            let handle = unsafe {
                CreateNamedPipeW(
                    path.as_ptr(),
                    PIPE_ACCESS_DUPLEX,
                    PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT,
                    PIPE_UNLIMITED_INSTANCES,
                    BUFFER_SIZE,
                    BUFFER_SIZE,
                    0,
                    ptr::null(),
                )
            };
            if handle == INVALID_HANDLE_VALUE {
                return Err(io::Error::last_os_error());
            }

            let ok = unsafe { ConnectNamedPipe(handle, ptr::null_mut()) };
            if ok == 0 {
                let err = unsafe { GetLastError() };
                if err != ERROR_PIPE_CONNECTED {
                    unsafe { CloseHandle(handle) };
                    return Err(io::Error::from_raw_os_error(err as i32));
                }
            }

            Ok(IpcStream {
                handle,
                _read_timeout: None,
            })
        }
    }

    impl Drop for IpcListener {
        fn drop(&mut self) {
            // Named pipes are kernel objects; no file to remove.
        }
    }

    pub struct IpcStream {
        handle: HANDLE,
        _read_timeout: Option<Duration>,
    }

    // HANDLE is safe to send across threads.
    unsafe impl Send for IpcStream {}

    impl IpcStream {
        pub fn connect() -> io::Result<Self> {
            let path = wide_path();
            let handle = unsafe {
                CreateFileW(
                    path.as_ptr(),
                    GENERIC_READ | GENERIC_WRITE,
                    0,
                    ptr::null(),
                    OPEN_EXISTING,
                    0,
                    ptr::null_mut(),
                )
            };
            if handle == INVALID_HANDLE_VALUE {
                return Err(io::Error::last_os_error());
            }
            Ok(Self {
                handle,
                _read_timeout: None,
            })
        }

        pub fn set_read_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
            // Synchronous named pipes don't support per-handle read timeouts.
            // Store the value for potential future use; this is a no-op.
            // We use interior mutability via a small trick: accept &self but
            // the stored value is only advisory anyway.
            let _ = timeout;
            Ok(())
        }

        pub fn shutdown_write(&self) -> io::Result<()> {
            let ok = unsafe { FlushFileBuffers(self.handle) };
            if ok == 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        }
    }

    impl Read for IpcStream {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            let mut bytes_read: u32 = 0;
            let ok = unsafe {
                ReadFile(
                    self.handle,
                    buf.as_mut_ptr() as _,
                    buf.len() as u32,
                    &mut bytes_read,
                    ptr::null_mut(),
                )
            };
            if ok == 0 {
                let err = io::Error::last_os_error();
                // ERROR_BROKEN_PIPE means EOF
                if err.raw_os_error() == Some(109) {
                    return Ok(0);
                }
                return Err(err);
            }
            Ok(bytes_read as usize)
        }
    }

    impl Write for IpcStream {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            let mut bytes_written: u32 = 0;
            let ok = unsafe {
                WriteFile(
                    self.handle,
                    buf.as_ptr() as _,
                    buf.len() as u32,
                    &mut bytes_written,
                    ptr::null_mut(),
                )
            };
            if ok == 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(bytes_written as usize)
        }

        fn flush(&mut self) -> io::Result<()> {
            let ok = unsafe { FlushFileBuffers(self.handle) };
            if ok == 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        }
    }

    impl Drop for IpcStream {
        fn drop(&mut self) {
            unsafe { CloseHandle(self.handle) };
        }
    }
}

// ─────────────────────────────────────────────
// Re-exports
// ─────────────────────────────────────────────
pub use platform::{IpcListener, IpcStream};

/// Clean up IPC resources (e.g. remove stale socket file).
pub fn cleanup() {
    #[cfg(unix)]
    {
        let _ = std::fs::remove_file(IPC_PATH);
    }
    #[cfg(windows)]
    {
        // Named pipes are kernel objects managed by the OS; nothing to clean up.
    }
}
