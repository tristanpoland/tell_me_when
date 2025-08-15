#[cfg(windows)]
use winapi::um::{
    fileapi::{CreateFileW, OPEN_EXISTING},
    handleapi::{CloseHandle, INVALID_HANDLE_VALUE},
    winbase::{FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OVERLAPPED, ReadDirectoryChangesW, INFINITE},
    winnt::{
        FILE_NOTIFY_CHANGE_ATTRIBUTES, FILE_NOTIFY_CHANGE_CREATION, FILE_NOTIFY_CHANGE_DIR_NAME,
        FILE_NOTIFY_CHANGE_FILE_NAME, FILE_NOTIFY_CHANGE_LAST_WRITE, FILE_NOTIFY_CHANGE_SIZE,
        FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, GENERIC_READ, HANDLE,
    },
    synchapi::{CreateEventW, WaitForSingleObject},
    errhandlingapi::GetLastError,
};
use winapi::shared::winerror::ERROR_IO_PENDING;
use winapi::um::minwinbase::OVERLAPPED;
use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use std::ptr;
use crate::handlers::fs::{FsWatchConfig, WatchHandle};
use crate::events::FsEventType;
use crate::{Result, TellMeWhenError};

pub struct WindowsWatchHandle {
    directory_handle: HANDLE,
    event_handle: HANDLE,
    buffer: Vec<u8>,
    overlapped: Box<OVERLAPPED>,
}

impl std::fmt::Debug for WindowsWatchHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WindowsWatchHandle")
            .field("directory_handle", &self.directory_handle)
            .field("event_handle", &self.event_handle)
            .field("buffer_len", &self.buffer.len())
            .finish()
    }
}

pub struct PlatformWatcher {
    handles: Vec<WindowsWatchHandle>,
}

unsafe impl Send for PlatformWatcher {}
unsafe impl Sync for PlatformWatcher {}

unsafe impl Send for WindowsWatchHandle {}
unsafe impl Sync for WindowsWatchHandle {}

impl PlatformWatcher {
    pub fn new() -> Result<Self> {
        Ok(Self {
            handles: Vec::new(),
        })
    }

    pub async fn watch_path(&mut self, path: &Path, config: &FsWatchConfig) -> Result<WatchHandle> {
        let wide_path: Vec<u16> = OsStr::new(path)
            .encode_wide()
            .chain(Some(0))
            .collect();

        unsafe {
            let directory_handle = CreateFileW(
                wide_path.as_ptr(),
                GENERIC_READ,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                ptr::null_mut(),
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OVERLAPPED,
                ptr::null_mut(),
            );

            if directory_handle == INVALID_HANDLE_VALUE {
                return Err(TellMeWhenError::System(
                    "Failed to open directory for watching".to_string(),
                ));
            }

            let event_handle = CreateEventW(ptr::null_mut(), 0, 0, ptr::null());
            if event_handle.is_null() {
                CloseHandle(directory_handle);
                return Err(TellMeWhenError::System(
                    "Failed to create event handle".to_string(),
                ));
            }

            let mut overlapped = Box::new(std::mem::zeroed::<OVERLAPPED>());
            overlapped.hEvent = event_handle;

            let buffer = vec![0u8; 4096];

            let watch_handle = WindowsWatchHandle {
                directory_handle,
                event_handle,
                buffer,
                overlapped,
            };

            // Start monitoring
            self.start_monitoring(&watch_handle, config)?;

            let handle = WatchHandle {
                handle: watch_handle,
            };

            Ok(handle)
        }
    }

    fn start_monitoring(&self, watch_handle: &WindowsWatchHandle, config: &FsWatchConfig) -> Result<()> {
        unsafe {
            let notify_filter = self.build_notify_filter(&config.event_types);
            let mut bytes_returned = 0u32;

            let success = ReadDirectoryChangesW(
                watch_handle.directory_handle,
                watch_handle.buffer.as_ptr() as *mut _,
                watch_handle.buffer.len() as u32,
                if config.watch_subdirectories { 1 } else { 0 },
                notify_filter,
                &mut bytes_returned,
                watch_handle.overlapped.as_ref() as *const _ as *mut _,
                None,
            );

            if success == 0 {
                let error = GetLastError();
                if error != ERROR_IO_PENDING {
                    return Err(TellMeWhenError::System(
                        format!("ReadDirectoryChangesW failed with error: {}", error),
                    ));
                }
            }
        }

        Ok(())
    }

    fn build_notify_filter(&self, event_types: &[FsEventType]) -> u32 {
        let mut filter = 0u32;

        for event_type in event_types {
            match event_type {
                FsEventType::Created => {
                    filter |= FILE_NOTIFY_CHANGE_CREATION | FILE_NOTIFY_CHANGE_FILE_NAME | FILE_NOTIFY_CHANGE_DIR_NAME;
                }
                FsEventType::Modified => {
                    filter |= FILE_NOTIFY_CHANGE_LAST_WRITE | FILE_NOTIFY_CHANGE_SIZE;
                }
                FsEventType::Deleted => {
                    filter |= FILE_NOTIFY_CHANGE_FILE_NAME | FILE_NOTIFY_CHANGE_DIR_NAME;
                }
                FsEventType::AttributeChanged => {
                    filter |= FILE_NOTIFY_CHANGE_ATTRIBUTES;
                }
                _ => {
                    // For renamed/moved events, we need file name changes
                    filter |= FILE_NOTIFY_CHANGE_FILE_NAME | FILE_NOTIFY_CHANGE_DIR_NAME;
                }
            }
        }

        if filter == 0 {
            // Default to all changes if no specific types specified
            filter = FILE_NOTIFY_CHANGE_FILE_NAME
                | FILE_NOTIFY_CHANGE_DIR_NAME
                | FILE_NOTIFY_CHANGE_LAST_WRITE
                | FILE_NOTIFY_CHANGE_CREATION
                | FILE_NOTIFY_CHANGE_SIZE
                | FILE_NOTIFY_CHANGE_ATTRIBUTES;
        }

        filter
    }

    pub async fn unwatch(&mut self, handle: WatchHandle) -> Result<()> {
        unsafe {
            CloseHandle(handle.handle.event_handle);
            CloseHandle(handle.handle.directory_handle);
        }
        Ok(())
    }
}

impl Drop for WindowsWatchHandle {
    fn drop(&mut self) {
        unsafe {
            if !self.event_handle.is_null() {
                CloseHandle(self.event_handle);
            }
            if self.directory_handle != INVALID_HANDLE_VALUE {
                CloseHandle(self.directory_handle);
            }
        }
    }
}