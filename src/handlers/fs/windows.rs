#[cfg(windows)]
use winapi::um::{
    fileapi::{CreateFileW, OPEN_EXISTING},
    handleapi::{CloseHandle, INVALID_HANDLE_VALUE},
    winbase::{FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OVERLAPPED, ReadDirectoryChangesW},
    ioapiset::{GetOverlappedResult, CancelIo},
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
use std::thread::{self, JoinHandle};

pub struct WindowsWatchHandle {
    directory_handle: HANDLE,
    event_handle: HANDLE,
    buffer: Vec<u8>,
    overlapped: Box<OVERLAPPED>,
    watched_path: PathBuf,
    event_sender: Option<Sender<EventMessage>>,
    handler_id: String,
    // Add a thread handle to manage the watcher thread
    worker_thread: Option<JoinHandle<()>>, 
}

impl Clone for WindowsWatchHandle {
    fn clone(&self) -> Self {
        WindowsWatchHandle {
            directory_handle: self.directory_handle,
            event_handle: self.event_handle,
            buffer: self.buffer.clone(),
            overlapped: Box::new(*self.overlapped),
            watched_path: self.watched_path.clone(),
            event_sender: self.event_sender.clone(),
            handler_id: self.handler_id.clone(),
            worker_thread: None, // Do not clone the thread handle
        }
    }
}

impl std::fmt::Debug for WindowsWatchHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WindowsWatchHandle")
            .field("directory_handle", &self.directory_handle)
            .field("event_handle", &self.event_handle)
            .field("buffer_len", &self.buffer.len())
            .field("watched_path", &self.watched_path)
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
        log::info!("Callback triggered with code: {}, bytes: {}", error_code, bytes_transferred);

        let context_ptr = (*overlapped).hEvent as *mut CallbackContext;
        if context_ptr.is_null() {
            log::error!("Completion routine called with null context pointer. This is a critical error.");
            return;
        }
        let context = &mut *context_ptr;

        if error_code != ERROR_SUCCESS && error_code != ERROR_IO_PENDING {
            log::error!("Filesystem monitoring stopped for path {:?} due to error: {}", context.watched_path, error_code);
            let _ = Box::from_raw(context_ptr);
            return;
        }
        
        if bytes_transferred > 0 {
            log::debug!("Processing {} bytes of notifications for path {:?}", bytes_transferred, context.watched_path);
            let buffer_slice = std::slice::from_raw_parts(context.buffer, bytes_transferred as usize);
            PlatformWatcher::process_notifications(
                buffer_slice,
                &context.watched_path,
                &context.event_sender,
                &context.handler_id,
            );
        }

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
                log::error!("Failed to restart ReadDirectoryChangesW in callback for path {:?}: {}", context.watched_path, error);
                let _ = Box::from_raw(context_ptr);
            } else {
                log::debug!("ReadDirectoryChangesW re-armed for path {:?}.", context.watched_path);
            }
        }
    }
}

impl PlatformWatcher {
    pub fn new(handler_id: String, event_sender: Option<Sender<EventMessage>>) -> Result<Self> {
        log::info!("PlatformWatcher created for handler_id: {}", handler_id);
        Ok(Self {
            handles: Vec::new(),
            event_sender,
            handler_id,
        })
    }

    pub fn run(&mut self) -> Result<()> {
        log::info!("Starting Windows watcher threads for {} handles...", self.handles.len());
        
        // This method is no longer a simple blocking call. It starts threads
        // for each handle and then blocks, waiting for them.
        for handle in &mut self.handles {
            let directory_handle = handle.directory_handle;
            let watched_path = handle.watched_path.clone();

            let worker_thread = thread::spawn(move || {
                log::info!("Worker thread started for path {:?}", watched_path);
                // This is the thread that will be "alerted" by the OS
                // when an I/O completion routine is ready.
                unsafe {
                    SleepEx(winapi::um::winbase::INFINITE, 1);
                }
                log::info!("Worker thread ending for path {:?}", watched_path);
            });

            handle.worker_thread = Some(worker_thread);
        }

        // The main thread needs to return control to the caller so they can
        // do other things. The worker threads are now managing the watches.
        // A future improvement might be to join these threads in a graceful shutdown process.
        
        Ok(())
    }

    pub async fn watch_path(&mut self, path: &Path, config: &FsWatchConfig) -> Result<WatchHandle> {
        log::info!("Watching path: {:?} with config: {:?}", path, config);
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
                let err_code = GetLastError();
                log::error!("Failed to open directory {:?} for watching. Error: {}", path, err_code);
                return Err(TellMeWhenError::System(
                    format!("Failed to open directory for watching: {}", err_code),
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
                worker_thread: None,
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

            self.start_monitoring(&mut watch_handle, config).await?;
            self.handles.push(watch_handle);

            // Return a handle that identifies this watcher.
            // Clone the last handle for WatchHandle.
            let last_handle = self.handles.last().unwrap().clone();
            Ok(WatchHandle { handle: last_handle })
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
                    log::error!("Initial ReadDirectoryChangesW failed for path {:?}. Error: {}", watch_handle.watched_path, error);
                    return Err(TellMeWhenError::System(
                        format!("ReadDirectoryChangesW failed with error: {}", error),
                    ));
                } else {
                    log::info!("Initial ReadDirectoryChangesW successfully queued for path {:?}.", watch_handle.watched_path);
                }
            }
        }
        Ok(())
    }

    fn build_notify_filter_static(event_types: &[FsEventType]) -> u32 {
        let mut filter = 0u32;
        for event_type in event_types {
            match event_type {
                FsEventType::Created => { filter |= FILE_NOTIFY_CHANGE_CREATION | FILE_NOTIFY_CHANGE_FILE_NAME | FILE_NOTIFY_CHANGE_DIR_NAME; }
                FsEventType::Modified => { filter |= FILE_NOTIFY_CHANGE_LAST_WRITE | FILE_NOTIFY_CHANGE_SIZE | FILE_NOTIFY_CHANGE_ATTRIBUTES; }
                FsEventType::Deleted => { filter |= FILE_NOTIFY_CHANGE_FILE_NAME | FILE_NOTIFY_CHANGE_DIR_NAME; }
                FsEventType::AttributeChanged => { filter |= FILE_NOTIFY_CHANGE_ATTRIBUTES; }
                FsEventType::Renamed { .. } => { filter |= FILE_NOTIFY_CHANGE_FILE_NAME | FILE_NOTIFY_CHANGE_DIR_NAME; }
                _ => {}
            }
        }
        if filter == 0 {
            filter = FILE_NOTIFY_CHANGE_FILE_NAME | FILE_NOTIFY_CHANGE_DIR_NAME | FILE_NOTIFY_CHANGE_LAST_WRITE | FILE_NOTIFY_CHANGE_CREATION | FILE_NOTIFY_CHANGE_SIZE | FILE_NOTIFY_CHANGE_ATTRIBUTES;
        }
        filter
    }

    fn process_notifications(buffer: &[u8], base_path: &Path, event_sender: &Option<Sender<EventMessage>>, handler_id: &str) {
        log::debug!("Processing notifications, buffer size: {}", buffer.len());
        
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
                        let filename_str = filename.to_string_lossy().trim_end_matches('\0').to_string();
                        let full_path = base_path.join(&filename_str);
                        let timestamp = SystemTime::now();

                        log::debug!("Found notification: Action={}, Path={:?}", info.Action, full_path);
                        
                        let event_type = match info.Action {
                            FILE_ACTION_ADDED => FsEventType::Created,
                            FILE_ACTION_REMOVED => FsEventType::Deleted,
                            FILE_ACTION_MODIFIED => FsEventType::Modified,
                            FILE_ACTION_RENAMED_OLD_NAME => {
                                // For simplicity and debugging, we'll log this but not create an event yet.
                                log::debug!("Found FILE_ACTION_RENAMED_OLD_NAME for {:?}", full_path);
                                continue; // Skip to next notification
                            },
                            FILE_ACTION_RENAMED_NEW_NAME => {
                                log::debug!("Found FILE_ACTION_RENAMED_NEW_NAME for {:?}", full_path);
                                FsEventType::Renamed {
                                    old_path: PathBuf::from("dummy_old_path"), // Placeholder
                                    new_path: full_path.clone(),
                                }
                            }
                            _ => FsEventType::Modified,
                        };
                        
                        let event_data = FsEventData {
                            event_type,
                            path: full_path,
                            timestamp,
                        };
                        send_event(sender, handler_id, event_data);
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
        // Find the handle and remove it.
        // Assuming WatchHandle now contains enough info to identify the correct WindowsWatchHandle
        // A simple implementation would be to just close all handles.
        while let Some(mut watch_handle) = self.handles.pop() {
            unsafe {
                let _ = CancelIo(watch_handle.directory_handle);
                // Join the worker thread to ensure it has finished.
                if let Some(worker_thread) = watch_handle.worker_thread.take() {
                    let _ = worker_thread.join();
                }

                let context_ptr = watch_handle.overlapped.hEvent as *mut CallbackContext;
                if !context_ptr.is_null() {
                    let _ = Box::from_raw(context_ptr);
                }

                if !watch_handle.event_handle.is_null() {
                    CloseHandle(watch_handle.event_handle);
                }
                if watch_handle.directory_handle != INVALID_HANDLE_VALUE {
                    CloseHandle(watch_handle.directory_handle);
                }
            }
        }
        Ok(())
    }
}

impl Drop for WindowsWatchHandle {
    fn drop(&mut self) {
        // The unwatch method should be called for proper cleanup.
        // Drop is for emergency cleanup if unwatch is not called.
        unsafe {
            let _ = CancelIo(self.directory_handle);
            if let Some(worker_thread) = self.worker_thread.take() {
                let _ = worker_thread.join();
            }

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