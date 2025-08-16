use crate::events::{EventData, ProcessEventData, ProcessEventType};
use crate::traits::{EventHandler, EventHandlerConfig, ThresholdConfig, IntervalConfig};
use crate::{EventMessage, EventMetadata, HandlerId, Result, TellMeWhenError};
use crossbeam_channel::Sender;
use sysinfo::{System, Pid, Process};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};
use async_trait::async_trait;

#[cfg(windows)]
mod windows;
#[cfg(unix)]
mod unix;
#[cfg(target_os = "macos")]
mod macos;

#[derive(Debug, Clone)]
pub struct ProcessConfig {
    pub base: EventHandlerConfig,
    pub cpu_threshold: f32,
    pub memory_threshold: u64, // in bytes
    pub monitor_new_processes: bool,
    pub monitor_terminated_processes: bool,
    pub process_name_filters: Vec<String>,
}

impl Default for ProcessConfig {
    fn default() -> Self {
        Self {
            base: EventHandlerConfig::default(),
            cpu_threshold: 80.0, // 80% CPU usage
            memory_threshold: 1_000_000_000, // 1GB
            monitor_new_processes: true,
            monitor_terminated_processes: true,
            process_name_filters: Vec::new(),
        }
    }
}

impl ThresholdConfig for ProcessConfig {
    fn set_threshold(&mut self, threshold: f32) {
        self.cpu_threshold = threshold;
    }

    fn get_threshold(&self) -> f32 {
        self.cpu_threshold
    }
}

impl IntervalConfig for ProcessConfig {
    fn set_interval(&mut self, interval: Duration) {
        self.base.poll_interval = interval;
    }

    fn get_interval(&self) -> Duration {
        self.base.poll_interval
    }
}

pub struct ProcessHandler {
    config: ProcessConfig,
    system: Arc<Mutex<System>>,
    previous_processes: Arc<Mutex<HashMap<u32, ProcessSnapshot>>>,
    is_running: Arc<Mutex<bool>>,
}

#[derive(Debug, Clone)]
struct ProcessSnapshot {
    pid: u32,
    name: String,
    cpu_usage: f32,
    memory_usage: u64,
    last_seen: SystemTime,
}

impl ProcessHandler {
    pub fn new(config: ProcessConfig) -> Self {
        Self {
            config,
            system: Arc::new(Mutex::new(System::new_all())),
            previous_processes: Arc::new(Mutex::new(HashMap::new())),
            is_running: Arc::new(Mutex::new(false)),
        }
    }

    #[cfg(windows)]
    async fn start_platform_specific(&self, sender: Sender<EventMessage>, handler_id: HandlerId) -> Result<()> {
        windows::start_process_monitoring(
            &self.config,
            &self.system,
            &self.previous_processes,
            &self.is_running,
            sender,
            handler_id,
        ).await
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    async fn start_platform_specific(&self, sender: Sender<EventMessage>, handler_id: HandlerId) -> Result<()> {
        unix::start_process_monitoring(
            &self.config,
            &self.system,
            &self.previous_processes,
            &self.is_running,
            sender,
            handler_id,
        ).await
    }

    #[cfg(target_os = "macos")]
    async fn start_platform_specific(&self, sender: Sender<EventMessage>, handler_id: HandlerId) -> Result<()> {
        macos::start_process_monitoring(
            &self.config,
            &self.system,
            &self.previous_processes,
            &self.is_running,
            sender,
            handler_id,
        ).await
    }

    fn emit_process_event(
        event_type: ProcessEventType,
        pid: u32,
        name: String,
        cpu_usage: Option<f32>,
        memory_usage: Option<u64>,
        sender: &Sender<EventMessage>,
        handler_id: &HandlerId,
    ) {
        let event_data = ProcessEventData {
            event_type,
            pid,
            name,
            cpu_usage,
            memory_usage,
            timestamp: SystemTime::now(),
        };

        let event_message = EventMessage {
            event_data: EventData::Process(event_data),
            metadata: EventMetadata {
                handler_id: handler_id.clone(),
                event_id: uuid::Uuid::new_v4().to_string(),
                timestamp: SystemTime::now(),
            },
        };

        if let Err(e) = sender.send(event_message) {
            log::error!("Failed to send process event: {}", e);
        }
    }
}

#[async_trait]
impl EventHandler for ProcessHandler {
    async fn start(&mut self, sender: Sender<EventMessage>, handler_id: HandlerId) -> Result<()> {
        {
            let mut is_running = self.is_running.lock().unwrap();
            if *is_running {
                return Ok(());
            }
            *is_running = true;
        }

        log::info!("Starting process monitoring with native OS callbacks");
        self.start_platform_specific(sender, handler_id).await
    }

    async fn stop(&mut self) -> Result<()> {
        let mut is_running = self.is_running.lock().unwrap();
        *is_running = false;
        log::info!("Process monitoring stopped");
        Ok(())
    }

    fn get_config(&self) -> &dyn std::any::Any {
        &self.config
    }

    fn set_config(&mut self, config: Box<dyn std::any::Any>) -> Result<()> {
        if let Ok(process_config) = config.downcast::<ProcessConfig>() {
            self.config = *process_config;
            Ok(())
        } else {
            Err(TellMeWhenError::ConfigError("Invalid config type for ProcessHandler".to_string()))
        }
    }
}