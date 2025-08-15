use crate::events::{EventData, FsEventData, FsEventType};
use crate::traits::{EventHandler, EventHandlerConfig};
use crate::{EventBus, EventMessage, EventMetadata, HandlerId, Result, TellMeWhenError};
use crossbeam_channel::Sender;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};
use tokio::time::interval;

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

        if let Some(watcher) = &mut self.platform_watcher {
            let handle = watcher.watch_path(&path, &self.config).await?;
            let mut watched_paths = self.watched_paths.lock().unwrap();
            watched_paths.insert(path, handle);
        }

        Ok(())
    }

    pub async fn unwatch_path<P: AsRef<Path>>(&mut self, path: P) -> Result<()> {
        let path = path.as_ref().to_path_buf();
        let handle = {
            let mut watched_paths = self.watched_paths.lock().unwrap();
            watched_paths.remove(&path)
        };
        
        if let Some(handle) = handle {
            if let Some(watcher) = &mut self.platform_watcher {
                watcher.unwatch(handle).await?;
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

    fn emit_event(&self, event_type: FsEventType, path: PathBuf) {
        if self.should_ignore_path(&path) {
            return;
        }

        if let Some(sender) = &self.event_sender {
            let event_data = FsEventData {
                event_type,
                path,
                timestamp: SystemTime::now(),
            };

            let message = EventMessage {
                metadata: EventMetadata {
                    id: 0, // Will be set by event bus
                    handler_id: self.handler_id.clone(),
                    timestamp: SystemTime::now(),
                    source: "filesystem".to_string(),
                },
                data: EventData::FileSystem(event_data),
            };

            if let Err(e) = sender.send(message) {
                log::error!("Failed to send filesystem event: {}", e);
            }
        }
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
        self.platform_watcher = Some(PlatformWatcher::new(
            self.handler_id.clone(),
            self.event_sender.clone()
        )?);
        self.is_running = true;

        log::info!("FileSystem handler started with id: {}", self.handler_id);
        Ok(())
    }

    async fn stop(&mut self) -> Result<()> {
        if !self.is_running {
            return Ok(());
        }

        // Stop all watches
        let watched_paths: Vec<PathBuf> = {
            let watched_paths = self.watched_paths.lock().unwrap();
            watched_paths.keys().cloned().collect()
        };

        for path in watched_paths {
            if let Err(e) = self.unwatch_path(&path).await {
                log::error!("Failed to unwatch path {:?}: {}", path, e);
            }
        }

        self.platform_watcher = None;
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

// Platform-specific handle type
#[derive(Debug)]
pub struct WatchHandle {
    #[cfg(windows)]
    pub(crate) handle: windows::WindowsWatchHandle,
    #[cfg(all(unix, not(target_os = "macos")))]
    pub(crate) handle: unix::UnixWatchHandle,
    #[cfg(target_os = "macos")]
    pub(crate) handle: macos::MacOsWatchHandle,
}