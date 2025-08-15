#[cfg(windows)]
use winapi::um::{
    fileapi::{CreateFileW, OPEN_EXISTING},
    handleapi::{CloseHandle, INVALID_HANDLE_VALUE},
    winbase::{FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OVERLAPPED, ReadDirectoryChangesW, INFINITE},
    winnt::{
        FILE_NOTIFY_CHANGE_ATTRIBUTES, FILE_NOTIFY_CHANGE_CREATION, FILE_NOTIFY_CHANGE_DIR_NAME,
        FILE_NOTIFY_CHANGE_FILE_NAME, FILE_NOTIFY_CHANGE_LAST_WRITE, FILE_NOTIFY_CHANGE_SIZE,
        FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, GENERIC_READ, HANDLE,
        FILE_ACTION_ADDED, FILE_ACTION_REMOVED, FILE_ACTION_MODIFIED, FILE_ACTION_RENAMED_OLD_NAME,
        FILE_ACTION_RENAMED_NEW_NAME,
    },
    synchapi::{CreateEventW, WaitForSingleObject},
    errhandlingapi::GetLastError,
};
use winapi::shared::winerror::ERROR_IO_PENDING;
use winapi::um::minwinbase::OVERLAPPED;
use winapi::um::winnt::FILE_NOTIFY_INFORMATION;
use winapi::um::synchapi::SleepEx;
use std::ffi::OsStr;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use std::ptr;
use std::mem;
use winapi::ctypes::c_void;
use crate::handlers::fs::{FsWatchConfig, WatchHandle};
use crate::events::{FsEventData, FsEventType};
use crate::{Result, TellMeWhenError};
use crossbeam_channel::Sender;
use crate::{EventMessage, EventData, EventMetadata};
use std::time::SystemTime;

pub struct WindowsWatchHandle {
    directory_handle: HANDLE,
    event_handle: HANDLE,
    buffer: Vec<u8>,
    overlapped: Box<OVERLAPPED>,
    watched_path: PathBuf,
    event_sender: Option<Sender<EventMessage>>,
    handler_id: String,
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
    event_sender: Option<Sender<EventMessage>>,
    handler_id: String,
}

unsafe impl Send for PlatformWatcher {}
unsafe impl Sync for PlatformWatcher {}

unsafe impl Send for WindowsWatchHandle {}
unsafe impl Sync for WindowsWatchHandle {}

// Context structure for the completion callback
struct CallbackContext {
    watched_path: PathBuf,
    event_sender: Option<Sender<EventMessage>>,
    handler_id: String,
    directory_handle: HANDLE,
    buffer: *mut u8,
    buffer_len: usize,
    config: FsWatchConfig,
}

// Windows completion routine called directly by the OS when filesystem events occur
extern "system" fn filesystem_completion_routine(
    _error_code: u32,
    bytes_transferred: u32,
    overlapped: *mut OVERLAPPED,
) {
    unsafe {
        if overlapped.is_null() {
            return;
        }

        // Get the context from the overlapped structure
        let context = &*((*overlapped).hEvent as *const CallbackContext);
        
        if bytes_transferred > 0 {
            let buffer_slice = std::slice::from_raw_parts(context.buffer, bytes_transferred as usize);
            PlatformWatcher::process_notifications(
                buffer_slice,
                &context.watched_path,
                &context.event_sender,
                &context.handler_id,
            );
        }

        // Restart monitoring for next batch of events
        let notify_filter = FILE_NOTIFY_CHANGE_FILE_NAME
            | FILE_NOTIFY_CHANGE_DIR_NAME
            | FILE_NOTIFY_CHANGE_LAST_WRITE
            | FILE_NOTIFY_CHANGE_CREATION
            | FILE_NOTIFY_CHANGE_SIZE
            | FILE_NOTIFY_CHANGE_ATTRIBUTES;

        let mut new_overlapped = std::mem::zeroed::<OVERLAPPED>();
        new_overlapped.hEvent = context as *const _ as *mut c_void;
        let mut bytes_returned = 0u32;

        let success = ReadDirectoryChangesW(
            context.directory_handle,
            context.buffer as *mut c_void,
            context.buffer_len as u32,
            if context.config.watch_subdirectories { 1 } else { 0 },
            notify_filter,
            &mut bytes_returned,
            &mut new_overlapped,
            Some(filesystem_completion_routine),
        );

        if success == 0 {
            let error = GetLastError();
            if error != ERROR_IO_PENDING {
                log::error!("Failed to restart ReadDirectoryChangesW in callback: {}", error);
            }
        }
    }
}

impl PlatformWatcher {
    pub fn new(handler_id: String, event_sender: Option<Sender<EventMessage>>) -> Result<Self> {
        Ok(Self {
            handles: Vec::new(),
            event_sender,
            handler_id,
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

            let mut watch_handle = WindowsWatchHandle {
                directory_handle,
                event_handle,
                buffer,
                overlapped,
                watched_path: path.to_path_buf(),
                event_sender: self.event_sender.clone(),
                handler_id: self.handler_id.clone(),
            };

            // Start the monitoring task
            self.start_monitoring_task(&watch_handle, config.clone()).await;

            let handle = WatchHandle {
                handle: watch_handle,
            };

            Ok(handle)
        }
    }

    fn start_monitoring(&self, watch_handle: &mut WindowsWatchHandle, config: &FsWatchConfig) -> Result<()> {
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

    async fn start_monitoring_task(&self, watch_handle: &WindowsWatchHandle, config: FsWatchConfig) {
        // Use ReadDirectoryChangesW with a completion routine (callback)
        let directory_handle_raw = watch_handle.directory_handle as usize;
        let watched_path = watch_handle.watched_path.clone();
        let event_sender = watch_handle.event_sender.clone();
        let handler_id = watch_handle.handler_id.clone();
        
        std::thread::spawn(move || {
            let directory_handle = directory_handle_raw as HANDLE;
            let mut buffer = vec![0u8; 4096];
            
            unsafe {
                // Use ReadDirectoryChangesW with a completion routine for immediate callbacks
                let mut overlapped = std::mem::zeroed::<OVERLAPPED>();
                let mut bytes_returned = 0u32;
                
                let notify_filter = FILE_NOTIFY_CHANGE_FILE_NAME
                    | FILE_NOTIFY_CHANGE_DIR_NAME
                    | FILE_NOTIFY_CHANGE_LAST_WRITE
                    | FILE_NOTIFY_CHANGE_CREATION
                    | FILE_NOTIFY_CHANGE_SIZE
                    | FILE_NOTIFY_CHANGE_ATTRIBUTES;

                // Set up context for the callback
                let context = CallbackContext {
                    watched_path: watched_path.clone(),
                    event_sender: event_sender.clone(),
                    handler_id: handler_id.clone(),
                    directory_handle,
                    buffer: buffer.as_mut_ptr(),
                    buffer_len: buffer.len(),
                    config: config.clone(),
                };
                
                overlapped.hEvent = &context as *const _ as *mut _;

                let success = ReadDirectoryChangesW(
                    directory_handle,
                    buffer.as_mut_ptr() as *mut _,
                    buffer.len() as u32,
                    if config.watch_subdirectories { 1 } else { 0 },
                    notify_filter,
                    &mut bytes_returned,
                    &mut overlapped,
                    Some(filesystem_completion_routine),
                );

                if success == 0 {
                    let error = GetLastError();
                    if error != ERROR_IO_PENDING {
                        log::error!("ReadDirectoryChangesW failed with error: {}", error);
                        return;
                    }
                }

                // Keep the thread alive and alertable for the completion routine
                loop {
                    SleepEx(1000, 1); // Alertable wait - allows completion routines to run
                }
            }
        });
    }

    fn process_notifications(buffer: &[u8], base_path: &Path, event_sender: &Option<Sender<EventMessage>>, handler_id: &str) {
        if let Some(sender) = event_sender {
            let mut offset = 0;
            
            while offset < buffer.len() {
                unsafe {
                    let info = &*(buffer.as_ptr().add(offset) as *const FILE_NOTIFY_INFORMATION);
                    
                    if info.FileNameLength > 0 {
                        let filename_slice = std::slice::from_raw_parts(
                            (buffer.as_ptr().add(offset + mem::size_of::<FILE_NOTIFY_INFORMATION>()) as *const u16),
                            (info.FileNameLength as usize) / 2
                        );
                        
                        let filename = std::ffi::OsString::from_wide(filename_slice);
                        let full_path = base_path.join(filename);
                        
                        let event_type = match info.Action {
                            FILE_ACTION_ADDED => FsEventType::Created,
                            FILE_ACTION_REMOVED => FsEventType::Deleted,
                            FILE_ACTION_MODIFIED => FsEventType::Modified,
                            FILE_ACTION_RENAMED_OLD_NAME => FsEventType::Deleted,
                            FILE_ACTION_RENAMED_NEW_NAME => FsEventType::Created,
                            _ => FsEventType::Modified,
                        };
                        
                        let event_data = FsEventData {
                            event_type,
                            path: full_path,
                            timestamp: SystemTime::now(),
                        };

                        let message = EventMessage {
                            metadata: EventMetadata {
                                id: 0, // Will be set by event bus
                                handler_id: handler_id.to_string(),
                                timestamp: SystemTime::now(),
                                source: "filesystem".to_string(),
                            },
                            data: EventData::FileSystem(event_data),
                        };

                        if let Err(e) = sender.send(message) {
                            log::error!("Failed to send filesystem event: {}", e);
                        }
                    }
                    
                    if info.NextEntryOffset == 0 {
                        break;
                    }
                    offset += info.NextEntryOffset as usize;
                }
            }
        }
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