#[cfg(windows)]
use winapi::um::{
    fileapi::{CreateFileW, OPEN_EXISTING},
    handleapi::{CloseHandle, INVALID_HANDLE_VALUE},
    winbase::{FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OVERLAPPED, ReadDirectoryChangesW},
    winnt::{
        FILE_NOTIFY_CHANGE_ATTRIBUTES, FILE_NOTIFY_CHANGE_CREATION, FILE_NOTIFY_CHANGE_DIR_NAME,
        FILE_NOTIFY_CHANGE_FILE_NAME, FILE_NOTIFY_CHANGE_LAST_WRITE, FILE_NOTIFY_CHANGE_SIZE,
        FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, FILE_LIST_DIRECTORY, HANDLE,
        FILE_ACTION_ADDED, FILE_ACTION_REMOVED, FILE_ACTION_MODIFIED, FILE_ACTION_RENAMED_OLD_NAME,
        FILE_ACTION_RENAMED_NEW_NAME,
    },
    synchapi::{CreateEventW, WaitForSingleObjectEx},
    ioapiset::CancelIo,
    minwinbase::{OVERLAPPED},
    errhandlingapi::GetLastError,
};
use std::ffi::{OsStr, OsString};
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use std::ptr;
use std::sync::{Arc, Mutex};
use std::time::SystemTime;
use std::mem;
use winapi::ctypes::c_void;

const BUF_SIZE: usize = 16384;

#[derive(Debug, Clone)]
pub enum FsEventKind {
    Created,
    Modified,
    Deleted,
    Renamed { old_path: PathBuf, new_path: PathBuf },
}

#[derive(Debug, Clone)]
pub struct FsEvent {
    pub kind: FsEventKind,
    pub path: PathBuf,
    pub timestamp: SystemTime,
}

type EventCallback = Arc<Mutex<dyn Fn(FsEvent) + Send + Sync>>;

#[derive(Clone)]
pub struct WatchData {
    pub dir: PathBuf,
    pub is_recursive: bool,
}

pub struct WatchRequest {
    pub buffer: Vec<u8>,
    pub handle: HANDLE,
    pub data: WatchData,
    pub event_callback: EventCallback,
    pub prev_rename: Mutex<Option<PathBuf>>,
}

unsafe extern "system" fn completion_routine(
    error_code: u32,
    bytes_transferred: u32,
    overlapped: *mut OVERLAPPED,
) {
    let req_ptr = (*overlapped).hEvent as *mut WatchRequest;
    if req_ptr.is_null() {
        return;
    }
    let req = &mut *req_ptr;
    if error_code != 0 { // 0 == ERROR_SUCCESS
        return;
    }
    if bytes_transferred > 0 {
        let buffer = &req.buffer[..bytes_transferred as usize];
        let mut offset = 0;
        while offset < buffer.len() {
            let info = &*(buffer.as_ptr().add(offset) as *const winapi::um::winnt::FILE_NOTIFY_INFORMATION);
            let filename_wide = std::slice::from_raw_parts(
                (buffer.as_ptr().add(offset + mem::size_of::<winapi::um::winnt::FILE_NOTIFY_INFORMATION>()) as *const u16),
                (info.FileNameLength as usize) / 2
            );
            let filename = OsString::from_wide(filename_wide);
            let full_path = req.data.dir.join(&filename);
            let timestamp = SystemTime::now();
            let event_kind = match info.Action {
                FILE_ACTION_ADDED => FsEventKind::Created,
                FILE_ACTION_REMOVED => FsEventKind::Deleted,
                FILE_ACTION_MODIFIED => FsEventKind::Modified,
                FILE_ACTION_RENAMED_OLD_NAME => {
                    let mut prev = req.prev_rename.lock().unwrap();
                    *prev = Some(full_path.clone());
                    offset += info.NextEntryOffset as usize;
                    if info.NextEntryOffset == 0 { break; }
                    continue;
                }
                FILE_ACTION_RENAMED_NEW_NAME => {
                    let old_path = req.prev_rename.lock().unwrap().take()
                        .unwrap_or_else(|| full_path.clone());
                    FsEventKind::Renamed { old_path, new_path: full_path.clone() }
                }
                _ => FsEventKind::Modified,
            };

            // Only emit event for non-old-name/renames
            if info.Action != FILE_ACTION_RENAMED_OLD_NAME {
                let event = FsEvent {
                    kind: event_kind,
                    path: full_path,
                    timestamp,
                };
                (req.event_callback.lock().unwrap())(event);
            }

            if info.NextEntryOffset == 0 { break; }
            offset += info.NextEntryOffset as usize;
        }
    }
    // Re-arm for next event
    let mut new_overlapped = mem::zeroed::<OVERLAPPED>();
    new_overlapped.hEvent = req_ptr as *mut c_void;
    let notify_filter = FILE_NOTIFY_CHANGE_FILE_NAME
        | FILE_NOTIFY_CHANGE_DIR_NAME
        | FILE_NOTIFY_CHANGE_LAST_WRITE
        | FILE_NOTIFY_CHANGE_CREATION
        | FILE_NOTIFY_CHANGE_SIZE
        | FILE_NOTIFY_CHANGE_ATTRIBUTES;
    let mut bytes_returned = 0u32;
    let _ = ReadDirectoryChangesW(
        req.handle,
        req.buffer.as_mut_ptr() as *mut c_void,
        req.buffer.len() as u32,
        if req.data.is_recursive { 1 } else { 0 },
        notify_filter,
        &mut bytes_returned,
        &mut new_overlapped,
        Some(completion_routine),
    );
}

pub struct WindowsFsWatcher {
    pub watch_req: Option<Box<WatchRequest>>,
}

impl WindowsFsWatcher {
    pub fn new<F>(path: &Path, recursive: bool, callback: F) -> Option<Self>
    where
        F: Fn(FsEvent) + Send + Sync + 'static,
    {
        let wide_path: Vec<u16> = OsStr::new(path).encode_wide().chain(Some(0)).collect();
        unsafe {
            let handle = CreateFileW(
                wide_path.as_ptr(),
                FILE_LIST_DIRECTORY,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                ptr::null_mut(),
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OVERLAPPED,
                ptr::null_mut(),
            );
            if handle == INVALID_HANDLE_VALUE {
                return None;
            }
            let watch_data = WatchData {
                dir: path.to_path_buf(),
                is_recursive: recursive,
            };
            let event_callback = Arc::new(Mutex::new(callback));
            let mut req = Box::new(WatchRequest {
                buffer: vec![0u8; BUF_SIZE],
                handle,
                data: watch_data,
                event_callback,
                prev_rename: Mutex::new(None),
            });
            let mut overlapped = mem::zeroed::<OVERLAPPED>();
            overlapped.hEvent = &mut *req as *mut _ as *mut c_void;
            let notify_filter = FILE_NOTIFY_CHANGE_FILE_NAME
                | FILE_NOTIFY_CHANGE_DIR_NAME
                | FILE_NOTIFY_CHANGE_LAST_WRITE
                | FILE_NOTIFY_CHANGE_CREATION
                | FILE_NOTIFY_CHANGE_SIZE
                | FILE_NOTIFY_CHANGE_ATTRIBUTES;
            let mut bytes_returned = 0u32;
            let result = ReadDirectoryChangesW(
                handle,
                req.buffer.as_mut_ptr() as *mut c_void,
                req.buffer.len() as u32,
                if recursive { 1 } else { 0 },
                notify_filter,
                &mut bytes_returned,
                &mut overlapped,
                Some(completion_routine),
            );
            if result == 0 {
                return None;
            }
            Some(WindowsFsWatcher {
                watch_req: Some(req),
            })
        }
    }

    /// Blocks in alertable wait. No polling, sleep, or loop required.
    pub fn run(&self) {
        unsafe {
            // SleepEx(INFINITE, TRUE) is required for completion routines
            winapi::um::synchapi::SleepEx(winapi::um::winbase::INFINITE, 1);
        }
    }

    pub fn stop(&mut self) {
        if let Some(req) = self.watch_req.take() {
            unsafe {
                CancelIo(req.handle);
                CloseHandle(req.handle);
            }
        }
    }
}