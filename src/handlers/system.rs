use crate::events::{EventData, SystemEventData, SystemEventType};
use crate::traits::{EventHandler, EventHandlerConfig, ThresholdConfig, IntervalConfig};
use crate::{EventBus, EventMessage, EventMetadata, HandlerId, Result, TellMeWhenError};
use crossbeam_channel::Sender;
use sysinfo::System;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};
use tokio::time::interval;

#[derive(Debug, Clone)]
pub struct SystemConfig {
    pub base: EventHandlerConfig,
    pub cpu_threshold: f32,
    pub memory_threshold: f32,
    pub disk_threshold: f32,
    pub temperature_threshold: f32,
    pub load_average_threshold: f32,
    pub monitor_cpu: bool,
    pub monitor_memory: bool,
    pub monitor_disk: bool,
    pub monitor_temperature: bool,
    pub monitor_load_average: bool,
}

impl Default for SystemConfig {
    fn default() -> Self {
        Self {
            base: EventHandlerConfig::default(),
            cpu_threshold: 80.0,
            memory_threshold: 85.0,
            disk_threshold: 90.0,
            temperature_threshold: 75.0, // Celsius
            load_average_threshold: 5.0,
            monitor_cpu: true,
            monitor_memory: true,
            monitor_disk: true,
            monitor_temperature: true,
            monitor_load_average: true,
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
    pub event_sender: Option<Sender<EventMessage>>,
    is_running: bool,
    handler_id: HandlerId,
    monitor_task: Option<tokio::task::JoinHandle<()>>,
}

impl SystemHandler {
    pub fn new(handler_id: HandlerId) -> Self {
        Self {
            config: SystemConfig::default(),
            system: Arc::new(Mutex::new(System::new_all())),
            event_sender: None,
            is_running: false,
            handler_id,
            monitor_task: None,
        }
    }

    pub fn with_config(handler_id: HandlerId, config: SystemConfig) -> Self {
        Self {
            config,
            system: Arc::new(Mutex::new(System::new_all())),
            event_sender: None,
            is_running: false,
            handler_id,
            monitor_task: None,
        }
    }

    fn start_monitoring(&mut self) {
        let system = self.system.clone();
        let config = self.config.clone();
        let event_sender = self.event_sender.clone();
        let handler_id = self.handler_id.clone();

        let task = tokio::spawn(async move {
            let mut interval = interval(config.base.poll_interval);
            
            loop {
                interval.tick().await;
                
                if let Some(sender) = &event_sender {
                    Self::check_system_metrics(
                        &system,
                        &config,
                        sender,
                        &handler_id,
                    ).await;
                }
            }
        });

        self.monitor_task = Some(task);
    }

    async fn check_system_metrics(
        system: &Arc<Mutex<System>>,
        config: &SystemConfig,
        sender: &Sender<EventMessage>,
        handler_id: &HandlerId,
    ) {
        let mut sys = system.lock().unwrap();
        sys.refresh_all();

        // Check CPU usage
        if config.monitor_cpu {
            let cpu_usage = sys.global_cpu_info().cpu_usage();
            if cpu_usage >= config.cpu_threshold {
                Self::emit_system_event(
                    SystemEventType::CpuUsageHigh,
                    Some(cpu_usage),
                    None,
                    None,
                    None,
                    None,
                    sender,
                    handler_id,
                );
            }
        }

        // Check memory usage
        if config.monitor_memory {
            let total_memory = sys.total_memory();
            let used_memory = sys.used_memory();
            let memory_usage = (used_memory as f32 / total_memory as f32) * 100.0;
            
            if memory_usage >= config.memory_threshold {
                Self::emit_system_event(
                    SystemEventType::MemoryUsageHigh,
                    None,
                    Some(memory_usage),
                    None,
                    None,
                    None,
                    sender,
                    handler_id,
                );
            }
        }

        // Note: disk, temperature, and load average monitoring
        // would require additional implementation for newer sysinfo versions
        // For now, we'll implement basic monitoring
    }

    fn emit_system_event(
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

        let message = EventMessage {
            metadata: EventMetadata {
                id: 0, // Will be set by event bus
                handler_id: handler_id.clone(),
                timestamp: SystemTime::now(),
                source: "system".to_string(),
            },
            data: EventData::System(event_data),
        };

        if let Err(e) = sender.send(message) {
            log::error!("Failed to send system event: {}", e);
        }
    }
}

#[async_trait::async_trait]
impl EventHandler for SystemHandler {
    type EventType = SystemEventData;
    type Config = SystemConfig;

    async fn start(&mut self, config: Self::Config) -> Result<()> {
        if self.is_running {
            return Ok(());
        }

        self.config = config;
        
        // Initialize system information
        {
            let mut sys = self.system.lock().unwrap();
            sys.refresh_all();
        }

        self.start_monitoring();
        self.is_running = true;

        log::info!("System handler started with id: {}", self.handler_id);
        Ok(())
    }

    async fn stop(&mut self) -> Result<()> {
        if !self.is_running {
            return Ok(());
        }

        if let Some(task) = self.monitor_task.take() {
            task.abort();
        }

        self.is_running = false;
        log::info!("System handler stopped: {}", self.handler_id);
        Ok(())
    }

    fn is_running(&self) -> bool {
        self.is_running
    }

    fn name(&self) -> &'static str {
        "system"
    }
}