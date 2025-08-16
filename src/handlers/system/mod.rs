use crate::events::{EventData, SystemEventData, SystemEventType};
use crate::traits::{EventHandlerConfig, ThresholdConfig, IntervalConfig};
use crate::{EventMessage, EventMetadata, HandlerId, Result, TellMeWhenError, EventId};
use crossbeam_channel::Sender;
use sysinfo::{System};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

#[cfg(windows)]
mod windows;
#[cfg(unix)]
mod unix;
#[cfg(target_os = "macos")]
mod macos;

#[derive(Debug, Clone)]
pub struct SystemConfig {
    pub base: EventHandlerConfig,
    pub cpu_threshold: f32,
    pub memory_threshold: f32,
    pub disk_threshold: f32,
    pub temperature_threshold: f32,
    pub load_threshold: f32,
    pub monitor_cpu: bool,
    pub monitor_memory: bool,
    pub monitor_disk: bool,
    pub monitor_temperature: bool,
    pub monitor_load: bool,
}

impl Default for SystemConfig {
    fn default() -> Self {
        Self {
            base: EventHandlerConfig::default(),
            cpu_threshold: 80.0, // 80% CPU usage
            memory_threshold: 80.0, // 80% memory usage
            disk_threshold: 90.0, // 90% disk usage
            temperature_threshold: 70.0, // 70°C
            load_threshold: 5.0, // Load average of 5.0
            monitor_cpu: true,
            monitor_memory: true,
            monitor_disk: true,
            monitor_temperature: true,
            monitor_load: true,
        }
    }
}

impl ThresholdConfig for SystemConfig {
    fn set_threshold(&mut self, threshold: f32) {
        self.cpu_threshold = threshold;
    }

    fn get_threshold(&self) -> f32 {
        self.cpu_threshold
    }
}

impl IntervalConfig for SystemConfig {
    fn set_interval(&mut self, interval: Duration) {
        self.base.poll_interval = interval;
    }

    fn get_interval(&self) -> Duration {
        self.base.poll_interval
    }
}

pub struct SystemHandler {
    config: SystemConfig,
    system: Arc<Mutex<System>>,
    is_running: Arc<Mutex<bool>>,
}

impl SystemHandler {
    pub fn new(config: SystemConfig) -> Self {
        Self {
            config,
            system: Arc::new(Mutex::new(System::new_all())),
            is_running: Arc::new(Mutex::new(false)),
        }
    }

    #[cfg(windows)]
    async fn start_platform_specific(&self, sender: Sender<EventMessage>, handler_id: HandlerId) -> Result<()> {
        windows::start_system_monitoring(
            &self.config,
            &self.system,
            &self.is_running,
            sender,
            handler_id,
        ).await
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    async fn start_platform_specific(&self, sender: Sender<EventMessage>, handler_id: HandlerId) -> Result<()> {
        unix::start_system_monitoring(
            &self.config,
            &self.system,
            &self.is_running,
            sender,
            handler_id,
        ).await
    }

    #[cfg(target_os = "macos")]
    async fn start_platform_specific(&self, sender: Sender<EventMessage>, handler_id: HandlerId) -> Result<()> {
        macos::start_system_monitoring(
            &self.config,
            &self.system,
            &self.is_running,
            sender,
            handler_id,
        ).await
    }

    pub fn emit_system_event(
        event_type: SystemEventType,
        cpu_usage: Option<f32>,
        memory_usage: Option<f32>,
        disk_usage: Option<f32>,
        temperature: Option<f32>,
        load_average: Option<f32>,
        sender: &Sender<EventMessage>,
        handler_id: &HandlerId,
    ) {
        let event_data = SystemEventData {
            event_type,
            cpu_usage,
            memory_usage,
            disk_usage,
            temperature,
            load_average,
            timestamp: SystemTime::now(),
        };

        let event_message = EventMessage {
            data: EventData::System(event_data),
            metadata: EventMetadata {
                id: uuid::Uuid::new_v4().as_u128() as EventId,
                handler_id: handler_id.clone(),
                timestamp: SystemTime::now(),
                source: "SystemHandler".to_string(),
            },
        };

        if let Err(e) = sender.send(event_message) {
            log::error!("Failed to send system event: {}", e);
        }
    }

    pub async fn start(&mut self, sender: Sender<EventMessage>, handler_id: HandlerId) -> Result<()> {
        {
            let mut is_running = self.is_running.lock().unwrap();
            if *is_running {
                return Ok(());
            }
            *is_running = true;
        }

        log::info!("Starting system monitoring with native OS callbacks");
        self.start_platform_specific(sender, handler_id).await
    }

    pub async fn stop(&mut self) -> Result<()> {
        let mut is_running = self.is_running.lock().unwrap();
        *is_running = false;
        log::info!("System monitoring stopped");
        Ok(())
    }
}