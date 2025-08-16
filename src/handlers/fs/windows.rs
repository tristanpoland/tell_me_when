#[cfg(windows)]
use winapi::um::{
    fileapi::{CreateFileW, OPEN_EXISTING},
    handleapi::{CloseHandle, INVALID_HANDLE_VALUE},
    winbase::{FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OVERLAPPED, ReadDirectoryChangesW},
    ioapiset::GetOverlappedResult,
    winnt::{
        FILE_NOTIFY_CHANGE_ATTRIBUTES, FILE_NOTIFY_CHANGE_CREATION, FILE_NOTIFY_CHANGE_DIR_NAME,
        FILE_NOTIFY_CHANGE_FILE_NAME, FILE_NOTIFY_CHANGE_LAST_WRITE, FILE_NOTIFY_CHANGE_SIZE,
        FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, GENERIC_READ, HANDLE,
        FILE_ACTION_ADDED, FILE_ACTION_REMOVED, FILE_ACTION_MODIFIED, FILE_ACTION_RENAMED_OLD_NAME,
        FILE_ACTION_RENAMED_NEW_NAME,
    },
    synchapi::{CreateEventW, WaitForSingleObject, SleepEx},
    errhandlingapi::GetLastError,
    minwinbase::{OVERLAPPED, LPOVERLAPPED_COMPLETION_ROUTINE},
};
use winapi::shared::winerror::{ERROR_IO_PENDING, ERROR_SUCCESS};
use winapi::um::winnt::FILE_NOTIFY_INFORMATION;
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

// Helper function to send the event message
fn send_event(sender: &Sender<EventMessage>, handler_id: &str, event_data: FsEventData) {
    let message = EventMessage {
        metadata: EventMetadata {
            id: 0,
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

// Windows completion routine called directly by the OS when filesystem events occur
extern "system" fn filesystem_completion_routine(
    error_code: u32,
    bytes_transferred: u32,
    overlapped: *mut OVERLAPPED,
) {
    unsafe {
        // Recover the context from the hEvent member of the OVERLAPPED structure
        let context_ptr = (*overlapped).hEvent as *mut CallbackContext;
        if context_ptr.is_null() {
            log::error!("Completion routine called with null context pointer.");
            return;
        }
        let context = &mut *context_ptr;

        if error_code != ERROR_SUCCESS && error_code != ERROR_IO_PENDING {
            log::error!("Filesystem monitoring stopped due to error: {}", error_code);
            let _ = Box::from_raw(context_ptr);
            return;
        }
        
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
        let notify_filter = PlatformWatcher::build_notify_filter_static(&context.config.event_types);
        let mut new_overlapped = std::mem::zeroed::<OVERLAPPED>();
        new_overlapped.hEvent = context_ptr as *mut c_void;
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
                let _ = Box::from_raw(context_ptr);
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

    pub fn run(&self) {
        log::info!("Windows watcher is now running and waiting for events...");
        unsafe {
            SleepEx(winapi::um::winbase::INFINITE, 1);
        }
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
                    format!("Failed to open directory for watching: {}", GetLastError()),
                ));
            }

            let event_handle = ptr::null_mut(); 

            let mut buffer = vec![0u8; 4096];
            let buffer_ptr = buffer.as_mut_ptr();
            let buffer_len = buffer.len();

            let mut watch_handle = WindowsWatchHandle {
                directory_handle,
                event_handle,
                buffer,
                overlapped: Box::new(std::mem::zeroed::<OVERLAPPED>()),
                watched_path: path.to_path_buf(),
                event_sender: self.event_sender.clone(),
                handler_id: self.handler_id.clone(),
            };

            let context = Box::new(CallbackContext {
                watched_path: watch_handle.watched_path.clone(),
                event_sender: watch_handle.event_sender.clone(),
                handler_id: watch_handle.handler_id.clone(),
                directory_handle: watch_handle.directory_handle,
                buffer: buffer_ptr,
                buffer_len,
                config: config.clone(),
            });

            watch_handle.overlapped.hEvent = Box::into_raw(context) as *mut c_void;

            self.start_monitoring(&mut watch_handle, config).await;

            let handle = WatchHandle {
                handle: watch_handle,
            };

            Ok(handle)
        }
    }

    async fn start_monitoring(&self, watch_handle: &mut WindowsWatchHandle, config: &FsWatchConfig) -> Result<()> {
        let notify_filter = Self::build_notify_filter_static(&config.event_types);
        let mut bytes_returned = 0u32;
        
        unsafe {
            let success = ReadDirectoryChangesW(
                watch_handle.directory_handle,
                watch_handle.buffer.as_ptr() as *mut _,
                watch_handle.buffer.len() as u32,
                if config.watch_subdirectories { 1 } else { 0 },
                notify_filter,
                &mut bytes_returned,
                watch_handle.overlapped.as_ref() as *const _ as *mut _,
                Some(filesystem_completion_routine),
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

    fn build_notify_filter_static(event_types: &[FsEventType]) -> u32 {
        let mut filter = 0u32;

        for event_type in event_types {
            match event_type {
                FsEventType::Created => {
                    filter |= FILE_NOTIFY_CHANGE_CREATION | FILE_NOTIFY_CHANGE_FILE_NAME | FILE_NOTIFY_CHANGE_DIR_NAME;
                }
                FsEventType::Modified => {
                    filter |= FILE_NOTIFY_CHANGE_LAST_WRITE | FILE_NOTIFY_CHANGE_SIZE | FILE_NOTIFY_CHANGE_ATTRIBUTES;
                }
                FsEventType::Deleted => {
                    filter |= FILE_NOTIFY_CHANGE_FILE_NAME | FILE_NOTIFY_CHANGE_DIR_NAME;
                }
                FsEventType::AttributeChanged => {
                    filter |= FILE_NOTIFY_CHANGE_ATTRIBUTES;
                }
                FsEventType::Renamed { .. } => {
                    filter |= FILE_NOTIFY_CHANGE_FILE_NAME | FILE_NOTIFY_CHANGE_DIR_NAME;
                }
                _ => {
                    
                }
            }
        }

        if filter == 0 {
            filter = FILE_NOTIFY_CHANGE_FILE_NAME
                | FILE_NOTIFY_CHANGE_DIR_NAME
                | FILE_NOTIFY_CHANGE_LAST_WRITE
                | FILE_NOTIFY_CHANGE_CREATION
                | FILE_NOTIFY_CHANGE_SIZE
                | FILE_NOTIFY_CHANGE_ATTRIBUTES;
        }

        filter
    }

    fn process_notifications(buffer: &[u8], base_path: &Path, event_sender: &Option<Sender<EventMessage>>, handler_id: &str) {
        log::debug!("Processing filesystem notifications, buffer size: {}", buffer.len());
        
        if let Some(sender) = event_sender {
            let mut offset = 0;
            let mut old_name_info: Option<(PathBuf, SystemTime)> = None;
            
            while offset < buffer.len() {
                unsafe {
                    let info = &*(buffer.as_ptr().add(offset) as *const FILE_NOTIFY_INFORMATION);
                    
                    if info.FileNameLength > 0 {
                        let filename_slice = std::slice::from_raw_parts(
                            (buffer.as_ptr().add(offset + mem::size_of::<FILE_NOTIFY_INFORMATION>()) as *const u16),
                            (info.FileNameLength as usize) / 2
                        );
                        
                        let filename = std::ffi::OsString::from_wide(filename_slice);
                        let filename_str = filename.to_string_lossy().trim_end_matches('\0').to_string();
                        let full_path = base_path.join(&filename_str);
                        let timestamp = SystemTime::now();
                        
                        match info.Action {
                            FILE_ACTION_ADDED => {
                                let event_data = FsEventData { event_type: FsEventType::Created, path: full_path, timestamp };
                                send_event(sender, handler_id, event_data);
                            }
                            FILE_ACTION_REMOVED => {
                                let event_data = FsEventData { event_type: FsEventType::Deleted, path: full_path, timestamp };
                                send_event(sender, handler_id, event_data);
                            }
                            FILE_ACTION_MODIFIED => {
                                let event_data = FsEventData { event_type: FsEventType::Modified, path: full_path, timestamp };
                                send_event(sender, handler_id, event_data);
                            }
                            FILE_ACTION_RENAMED_OLD_NAME => {
                                old_name_info = Some((full_path, timestamp));
                            }
                            FILE_ACTION_RENAMED_NEW_NAME => {
                                if let Some((old_path, old_time)) = old_name_info.take() {
                                    let event_data = FsEventData {
                                        event_type: FsEventType::Renamed { old_path, new_path: full_path.clone() },
                                        path: full_path,
                                        timestamp: old_time,
                                    };
                                    send_event(sender, handler_id, event_data);
                                } else {
                                    log::warn!("Received FILE_ACTION_RENAMED_NEW_NAME without a preceding old name.");
                                    let event_data = FsEventData {
                                        event_type: FsEventType::Created,
                                        path: full_path,
                                        timestamp,
                                    };
                                    send_event(sender, handler_id, event_data);
                                }
                            }
                            _ => {
                                let event_data = FsEventData { event_type: FsEventType::Modified, path: full_path, timestamp };
                                send_event(sender, handler_id, event_data);
                            }
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
        Self::build_notify_filter_static(event_types)
    }

    pub async fn unwatch(&mut self, handle: WatchHandle) -> Result<()> {
        unsafe {
            let _ = winapi::um::ioapiset::CancelIo(handle.handle.directory_handle);
            
            let context_ptr = handle.handle.overlapped.hEvent as *mut CallbackContext;
            if !context_ptr.is_null() {
                let _ = Box::from_raw(context_ptr);
            }

            if !handle.handle.event_handle.is_null() {
                CloseHandle(handle.handle.event_handle);
            }
            if handle.handle.directory_handle != INVALID_HANDLE_VALUE {
                CloseHandle(handle.handle.directory_handle);
            }
        }
        Ok(())
    }
}

impl Drop for WindowsWatchHandle {
    fn drop(&mut self) {
        unsafe {
            let _ = winapi::um::ioapiset::CancelIo(self.directory_handle);
            
            let context_ptr = self.overlapped.hEvent as *mut CallbackContext;
            if !context_ptr.is_null() {
                let _ = Box::from_raw(context_ptr);
            }

            if !self.event_handle.is_null() {
                CloseHandle(self.event_handle);
            }
            if self.directory_handle != INVALID_HANDLE_VALUE {
                CloseHandle(self.directory_handle);
            }
        }
    }
}