#[cfg(target_os = "macos")]
use core_foundation::base::{CFRelease, CFRetain, CFTypeRef};
use core_foundation::string::{CFString, CFStringRef};
use core_foundation::array::{CFArray, CFArrayRef};
use core_foundation::runloop::{CFRunLoop, CFRunLoopRef, CFRunLoopRun, CFRunLoopStop};
use core_foundation_sys::base::{kCFAllocatorDefault, CFIndex};
use std::path::{Path, PathBuf};
use std::ffi::c_void;
use std::ptr;
use crate::handlers::fs::{FsWatchConfig, WatchHandle};
use crate::events::FsEventType;
use crate::{Result, TellMeWhenError};

#[derive(Debug)]
pub struct MacOsWatchHandle {
    stream_ref: FSEventStreamRef,
    run_loop: CFRunLoopRef,
    path: PathBuf,
}

pub struct PlatformWatcher {
    active_streams: Vec<FSEventStreamRef>,
}

unsafe impl Send for PlatformWatcher {}
unsafe impl Sync for PlatformWatcher {}

unsafe impl Send for MacOsWatchHandle {}
unsafe impl Sync for MacOsWatchHandle {}

// FFI declarations for FSEvents
#[repr(C)]
struct FSEventStreamRef(*const c_void);

unsafe impl Send for FSEventStreamRef {}
unsafe impl Sync for FSEventStreamRef {}

#[repr(C)]
struct FSEventStreamContext {
    version: CFIndex,
    info: *mut c_void,
    retain: Option<extern "C" fn(*const c_void) -> *const c_void>,
    release: Option<extern "C" fn(*const c_void)>,
    copy_description: Option<extern "C" fn(*const c_void) -> CFStringRef>,
}

type FSEventStreamCallback = extern "C" fn(
    stream_ref: FSEventStreamRef,
    client_callback_info: *mut c_void,
    num_events: usize,
    event_paths: *mut c_void,
    event_flags: *const u32,
    event_ids: *const u64,
);

extern "C" {
    fn FSEventStreamCreate(
        allocator: *const c_void,
        callback: FSEventStreamCallback,
        context: *mut FSEventStreamContext,
        path_to_watch: CFArrayRef,
        since_when: u64,
        latency: f64,
        flags: u32,
    ) -> FSEventStreamRef;

    fn FSEventStreamScheduleWithRunLoop(
        stream_ref: FSEventStreamRef,
        run_loop: CFRunLoopRef,
        run_loop_mode: CFStringRef,
    );

    fn FSEventStreamStart(stream_ref: FSEventStreamRef) -> bool;
    fn FSEventStreamStop(stream_ref: FSEventStreamRef);
    fn FSEventStreamInvalidate(stream_ref: FSEventStreamRef);
    fn FSEventStreamRelease(stream_ref: FSEventStreamRef);
}

const kFSEventStreamCreateFlagFileEvents: u32 = 0x00000010;
const kFSEventStreamCreateFlagNoDefer: u32 = 0x00000002;
const kFSEventStreamCreateFlagWatchRoot: u32 = 0x00000004;

const kFSEventStreamEventFlagItemCreated: u32 = 0x00000100;
const kFSEventStreamEventFlagItemRemoved: u32 = 0x00000200;
const kFSEventStreamEventFlagItemInodeMetaMod: u32 = 0x00000400;
const kFSEventStreamEventFlagItemRenamed: u32 = 0x00000800;
const kFSEventStreamEventFlagItemModified: u32 = 0x00001000;
const kFSEventStreamEventFlagItemFinderInfoMod: u32 = 0x00002000;
const kFSEventStreamEventFlagItemChangeOwner: u32 = 0x00004000;
const kFSEventStreamEventFlagItemXattrMod: u32 = 0x00008000;

impl PlatformWatcher {
    pub fn new() -> Result<Self> {
        Ok(Self {
            active_streams: Vec::new(),
        })
    }

    pub async fn watch_path(&mut self, path: &Path, config: &FsWatchConfig) -> Result<WatchHandle> {
        unsafe {
            let path_string = CFString::new(&path.to_string_lossy());
            let paths_array = CFArray::from_copyable(&[path_string]);

            let mut context = FSEventStreamContext {
                version: 0,
                info: ptr::null_mut(),
                retain: None,
                release: None,
                copy_description: None,
            };

            let stream_ref = FSEventStreamCreate(
                kCFAllocatorDefault,
                fs_event_callback,
                &mut context,
                paths_array.as_concrete_TypeRef(),
                0, // kFSEventStreamEventIdSinceNow
                0.1, // latency in seconds
                kFSEventStreamCreateFlagFileEvents 
                    | kFSEventStreamCreateFlagNoDefer 
                    | kFSEventStreamCreateFlagWatchRoot,
            );

            if stream_ref.0.is_null() {
                return Err(TellMeWhenError::System(
                    "Failed to create FSEventStream".to_string(),
                ));
            }

            let run_loop = CFRunLoop::get_current().as_concrete_TypeRef();
            let run_loop_mode = CFString::new("kCFRunLoopDefaultMode");

            FSEventStreamScheduleWithRunLoop(
                stream_ref,
                run_loop,
                run_loop_mode.as_concrete_TypeRef(),
            );

            if !FSEventStreamStart(stream_ref) {
                FSEventStreamRelease(stream_ref);
                return Err(TellMeWhenError::System(
                    "Failed to start FSEventStream".to_string(),
                ));
            }

            self.active_streams.push(stream_ref);

            let handle = WatchHandle {
                handle: MacOsWatchHandle {
                    stream_ref,
                    run_loop,
                    path: path.to_path_buf(),
                },
            };

            Ok(handle)
        }
    }

    pub async fn unwatch(&mut self, handle: WatchHandle) -> Result<()> {
        unsafe {
            let stream_ref = handle.handle.stream_ref;
            
            FSEventStreamStop(stream_ref);
            FSEventStreamInvalidate(stream_ref);
            FSEventStreamRelease(stream_ref);

            self.active_streams.retain(|&s| s.0 != stream_ref.0);
        }
        Ok(())
    }
}

extern "C" fn fs_event_callback(
    _stream_ref: FSEventStreamRef,
    _client_callback_info: *mut c_void,
    num_events: usize,
    event_paths: *mut c_void,
    event_flags: *const u32,
    _event_ids: *const u64,
) {
    unsafe {
        let paths = event_paths as *const *const i8;
        
        for i in 0..num_events {
            let path_ptr = *paths.add(i);
            let path_cstr = std::ffi::CStr::from_ptr(path_ptr);
            
            if let Ok(path_str) = path_cstr.to_str() {
                let path = PathBuf::from(path_str);
                let flags = *event_flags.add(i);
                
                let event_type = flags_to_event_type(flags);
                
                // In a real implementation, you'd emit the event here
                log::debug!("FSEvent: {:?} at {:?}", event_type, path);
            }
        }
    }
}

fn flags_to_event_type(flags: u32) -> FsEventType {
    if flags & kFSEventStreamEventFlagItemCreated != 0 {
        FsEventType::Created
    } else if flags & kFSEventStreamEventFlagItemRemoved != 0 {
        FsEventType::Deleted
    } else if flags & kFSEventStreamEventFlagItemRenamed != 0 {
        // For simplicity, treating renames as moves
        FsEventType::Renamed {
            old_path: PathBuf::new(),
            new_path: PathBuf::new(),
        }
    } else if flags & kFSEventStreamEventFlagItemModified != 0 {
        FsEventType::Modified
    } else if flags & (kFSEventStreamEventFlagItemInodeMetaMod 
                     | kFSEventStreamEventFlagItemFinderInfoMod 
                     | kFSEventStreamEventFlagItemXattrMod) != 0 {
        FsEventType::AttributeChanged
    } else if flags & kFSEventStreamEventFlagItemChangeOwner != 0 {
        FsEventType::PermissionChanged
    } else {
        FsEventType::Modified // Default fallback
    }
}

impl Drop for MacOsWatchHandle {
    fn drop(&mut self) {
        unsafe {
            FSEventStreamStop(self.stream_ref);
            FSEventStreamInvalidate(self.stream_ref);
            FSEventStreamRelease(self.stream_ref);
        }
    }
}