#[cfg(all(unix, not(target_os = "macos")))]
use inotify::{Inotify, WatchMask, Event, EventMask};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use crate::handlers::fs::{FsWatchConfig, WatchHandle};
use crate::events::FsEventType;
use crate::{Result, TellMeWhenError};

#[derive(Debug)]
pub struct UnixWatchHandle {
    watch_descriptor: i32,
    path: PathBuf,
}

pub struct PlatformWatcher {
    inotify: Inotify,
    watches: HashMap<i32, PathBuf>,
}

unsafe impl Send for PlatformWatcher {}
unsafe impl Sync for PlatformWatcher {}

unsafe impl Send for UnixWatchHandle {}
unsafe impl Sync for UnixWatchHandle {}

impl PlatformWatcher {
    pub fn new() -> Result<Self> {
        let inotify = Inotify::init()
            .map_err(|e| TellMeWhenError::System(format!("Failed to initialize inotify: {}", e)))?;

        Ok(Self {
            inotify,
            watches: HashMap::new(),
        })
    }

    pub async fn watch_path(&mut self, path: &Path, config: &FsWatchConfig) -> Result<WatchHandle> {
        let mask = self.build_watch_mask(&config.event_types);
        
        let watch_descriptor = self.inotify
            .add_watch(path, mask)
            .map_err(|e| TellMeWhenError::System(format!("Failed to add inotify watch: {}", e)))?;

        self.watches.insert(watch_descriptor, path.to_path_buf());

        // If we're watching subdirectories, recursively add watches
        if config.watch_subdirectories && path.is_dir() {
            self.add_recursive_watches(path, &mask)?;
        }

        let handle = WatchHandle {
            handle: UnixWatchHandle {
                watch_descriptor,
                path: path.to_path_buf(),
            },
        };

        // Start the event monitoring loop
        self.start_event_loop();

        Ok(handle)
    }

    fn add_recursive_watches(&mut self, dir_path: &Path, mask: &WatchMask) -> Result<()> {
        if let Ok(entries) = std::fs::read_dir(dir_path) {
            for entry in entries {
                if let Ok(entry) = entry {
                    let path = entry.path();
                    if path.is_dir() {
                        if let Ok(watch_descriptor) = self.inotify.add_watch(&path, *mask) {
                            self.watches.insert(watch_descriptor, path.clone());
                            // Recursively add subdirectories
                            self.add_recursive_watches(&path, mask)?;
                        }
                    }
                }
            }
        }
        Ok(())
    }

    fn build_watch_mask(&self, event_types: &[FsEventType]) -> WatchMask {
        let mut mask = WatchMask::empty();

        for event_type in event_types {
            match event_type {
                FsEventType::Created => {
                    mask |= WatchMask::CREATE;
                }
                FsEventType::Modified => {
                    mask |= WatchMask::MODIFY | WatchMask::CLOSE_WRITE;
                }
                FsEventType::Deleted => {
                    mask |= WatchMask::DELETE | WatchMask::DELETE_SELF;
                }
                FsEventType::Renamed { .. } | FsEventType::Moved { .. } => {
                    mask |= WatchMask::MOVED_FROM | WatchMask::MOVED_TO;
                }
                FsEventType::AttributeChanged => {
                    mask |= WatchMask::ATTRIB;
                }
                FsEventType::PermissionChanged => {
                    mask |= WatchMask::ATTRIB;
                }
            }
        }

        // Add some default useful events
        mask |= WatchMask::DONT_FOLLOW | WatchMask::EXCL_UNLINK;

        if mask.is_empty() {
            // Default mask if no specific types specified
            mask = WatchMask::CREATE 
                | WatchMask::MODIFY 
                | WatchMask::DELETE 
                | WatchMask::MOVED_FROM 
                | WatchMask::MOVED_TO 
                | WatchMask::CLOSE_WRITE;
        }

        mask
    }

    fn start_event_loop(&self) {
        let mut buffer = [0; 4096];
        
        tokio::spawn(async move {
            // This is a simplified event loop - in a real implementation,
            // you'd want to use tokio's async file I/O or run this in a separate thread
            loop {
                // Read events from inotify
                // Process and emit events
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
        });
    }

    pub async fn unwatch(&mut self, handle: WatchHandle) -> Result<()> {
        let watch_descriptor = handle.handle.watch_descriptor;
        
        self.inotify
            .rm_watch(watch_descriptor)
            .map_err(|e| TellMeWhenError::System(format!("Failed to remove inotify watch: {}", e)))?;

        self.watches.remove(&watch_descriptor);
        Ok(())
    }

    fn process_inotify_event(&self, event: Event<&std::ffi::OsStr>) -> Option<(FsEventType, PathBuf)> {
        let path = if let Some(watch_path) = self.watches.get(&event.wd) {
            if let Some(name) = event.name {
                watch_path.join(name)
            } else {
                watch_path.clone()
            }
        } else {
            return None;
        };

        let event_type = if event.mask.contains(EventMask::CREATE) {
            FsEventType::Created
        } else if event.mask.contains(EventMask::MODIFY) || event.mask.contains(EventMask::CLOSE_WRITE) {
            FsEventType::Modified
        } else if event.mask.contains(EventMask::DELETE) || event.mask.contains(EventMask::DELETE_SELF) {
            FsEventType::Deleted
        } else if event.mask.contains(EventMask::MOVED_FROM) || event.mask.contains(EventMask::MOVED_TO) {
            // For simplicity, treating moves as renames
            // In a full implementation, you'd track move pairs
            FsEventType::Renamed {
                old_path: path.clone(),
                new_path: path.clone(),
            }
        } else if event.mask.contains(EventMask::ATTRIB) {
            FsEventType::AttributeChanged
        } else {
            return None;
        };

        Some((event_type, path))
    }
}