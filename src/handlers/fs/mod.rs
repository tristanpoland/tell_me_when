use crate::events::{EventData, FsEventData, FsEventType};
use crate::traits::{EventHandler, EventHandlerConfig};
use crate::{EventMessage, EventMetadata, HandlerId, Result, TellMeWhenError};
use crossbeam_channel::Sender;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

#[cfg(windows)]
mod windows;
#[cfg(unix)]
mod unix;
#[cfg(target_os = "macos")]
mod macos;

#[cfg(windows)]
use windows::*;
#[cfg(all(unix, not(target_os = "macos")))]
use unix::*;
#[cfg(target_os = "macos")]
use macos::*;

#[derive(Debug, Clone)]
pub struct FsWatchConfig {
    pub base: EventHandlerConfig,
    pub watch_subdirectories: bool,
    pub ignore_patterns: Vec<String>,
    pub debounce_events: bool,
    pub event_types: Vec<FsEventType>,
}

impl Default for FsWatchConfig {
    fn default() -> Self {
        Self {
            base: EventHandlerConfig::default(),
            watch_subdirectories: true,
            ignore_patterns: vec![
                "*.tmp".to_string(),
                "*.swp".to_string(),
                ".git/*".to_string(),
                "node_modules/*".to_string(),
            ],
            debounce_events: true,
            event_types: vec![
                FsEventType::Created,
                FsEventType::Modified,
                FsEventType::Deleted,
            ],
        }
    }
}

pub struct FileSystemHandler {
    config: FsWatchConfig,
    watched_paths: Arc<Mutex<HashMap<PathBuf, WatchHandle>>>,
    pub event_sender: Option<Sender<EventMessage>>,
    is_running: bool,
    handler_id: HandlerId,
    #[cfg(windows)]
    platform_watcher: Option<Arc<WindowsFsWatcher>>,
    #[cfg(all(unix, not(target_os = "macos")))]
    platform_watcher: Option<PlatformWatcher>,
    #[cfg(target_os = "macos")]
    platform_watcher: Option<PlatformWatcher>,
}

unsafe impl Send for FileSystemHandler {}
unsafe impl Sync for FileSystemHandler {}

impl FileSystemHandler {
    pub fn new(handler_id: HandlerId) -> Self {
        Self {
            config: FsWatchConfig::default(),
            watched_paths: Arc::new(Mutex::new(HashMap::new())),
            event_sender: None,
            is_running: false,
            handler_id,
            platform_watcher: None,
        }
    }

    pub fn with_config(handler_id: HandlerId, config: FsWatchConfig) -> Self {
        Self {
            config,
            watched_paths: Arc::new(Mutex::new(HashMap::new())),
            event_sender: None,
            is_running: false,
            handler_id,
            platform_watcher: None,
        }
    }

    pub async fn watch_path<P: AsRef<Path>>(&mut self, path: P) -> Result<()> {
        let path = path.as_ref().to_path_buf();
        
        if !path.exists() {
            return Err(TellMeWhenError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("Path does not exist: {:?}", path),
            )));
        }

        #[cfg(windows)]
        {
            if self.platform_watcher.is_none() {
                self.platform_watcher = Some(Arc::new(WindowsFsWatcher::new()));
            }
            let watcher = self.platform_watcher.as_ref().unwrap().clone();
            let sender = self.event_sender.clone();
            let handler_id = self.handler_id.clone();
            let config = self.config.clone();
            let path_clone = path.clone();

            watcher.watch(
                &path,
                config.watch_subdirectories,
                move |event: FsEvent| {
                    let event_type = match event.kind {
                        FsEventKind::Created => FsEventType::Created,
                        FsEventKind::Modified => FsEventType::Modified,
                        FsEventKind::Deleted => FsEventType::Deleted,
                        FsEventKind::Renamed { old_path, new_path } => FsEventType::Renamed { old_path, new_path },
                    };
                    let fs_event_data = FsEventData {
                        event_type,
                        path: event.path,
                        timestamp: event.timestamp,
                    };
                    if let Some(sender) = &sender {
                        let message = EventMessage {
                            metadata: EventMetadata {
                                id: 0,
                                handler_id: handler_id.clone(),
                                timestamp: SystemTime::now(),
                                source: "filesystem".to_string(),
                            },
                            data: EventData::FileSystem(fs_event_data),
                        };
                        let _ = sender.send(message);
                    }
                }
            );
            let mut watched_paths = self.watched_paths.lock().unwrap();
            watched_paths.insert(path.clone(), WatchHandle { handle: 0 }); // handle not used here
        }

        // TODO: Implement for Unix/MacOS

        Ok(())
    }

    pub async fn unwatch_path<P: AsRef<Path>>(&mut self, path: P) -> Result<()> {
        let path = path.as_ref().to_path_buf();
        let handle = {
            let mut watched_paths = self.watched_paths.lock().unwrap();
            watched_paths.remove(&path)
        };

        #[cfg(windows)]
        {
            if let Some(watcher) = &self.platform_watcher {
                watcher.stop();
            }
        }

        Ok(())
    }

    fn should_ignore_path(&self, path: &Path) -> bool {
        let path_str = path.to_string_lossy();
        self.config.ignore_patterns.iter().any(|pattern| {
            if pattern.contains('*') {
                let regex_pattern = pattern.replace("*", ".*");
                if let Ok(regex) = regex::Regex::new(&regex_pattern) {
                    return regex.is_match(&path_str);
                }
            }
            path_str.contains(pattern)
        })
    }
}

#[async_trait::async_trait]
impl EventHandler for FileSystemHandler {
    type EventType = FsEventData;
    type Config = FsWatchConfig;

    async fn start(&mut self, config: Self::Config) -> Result<()> {
        if self.is_running {
            return Ok(());
        }

        self.config = config;
        #[cfg(windows)]
        {
            self.platform_watcher = Some(Arc::new(WindowsFsWatcher::new()));
        }
        self.is_running = true;

        log::info!("FileSystem handler started with id: {}", self.handler_id);
        Ok(())
    }

    async fn stop(&mut self) -> Result<()> {
        if !self.is_running {
            return Ok(());
        }

        let watched_paths: Vec<PathBuf> = {
            let watched_paths = self.watched_paths.lock().unwrap();
            watched_paths.keys().cloned().collect()
        };

        for path in watched_paths {
            let _ = self.unwatch_path(&path).await;
        }

        #[cfg(windows)]
        {
            if let Some(watcher) = &self.platform_watcher {
                watcher.stop();
            }
            self.platform_watcher = None;
        }

        self.is_running = false;

        log::info!("FileSystem handler stopped: {}", self.handler_id);
        Ok(())
    }

    fn is_running(&self) -> bool {
        self.is_running
    }

    fn name(&self) -> &'static str {
        "filesystem"
    }
}

#[derive(Debug)]
pub struct WatchHandle {
    #[cfg(windows)]
    pub(crate) handle: u64, // not used, but required for trait compatibility
    #[cfg(all(unix, not(target_os = "macos")))]
    pub(crate) handle: unix::UnixWatchHandle,
    #[cfg(target_os = "macos")]
    pub(crate) handle: macos::MacOsWatchHandle,
}