use crate::events::{EventData, NetworkEventData, NetworkEventType};
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
pub struct NetworkConfig {
    pub base: EventHandlerConfig,
    pub monitor_interface_changes: bool,
    pub monitor_connection_changes: bool,
    pub traffic_threshold_bytes: u64,
    pub interface_filters: Vec<String>,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            base: EventHandlerConfig::default(),
            monitor_interface_changes: true,
            monitor_connection_changes: true,
            traffic_threshold_bytes: 1_000_000_000, // 1GB
            interface_filters: Vec::new(),
        }
    }
}

impl ThresholdConfig for NetworkConfig {
    fn set_threshold(&mut self, threshold: f32) {
        self.traffic_threshold_bytes = threshold as u64;
    }

    fn get_threshold(&self) -> f32 {
        self.traffic_threshold_bytes as f32
    }
}

impl IntervalConfig for NetworkConfig {
    fn set_interval(&mut self, interval: Duration) {
        self.base.poll_interval = interval;
    }

    fn get_interval(&self) -> Duration {
        self.base.poll_interval
    }
}

pub struct NetworkHandler {
    config: NetworkConfig,
    system: Arc<Mutex<System>>,
    previous_interfaces: Arc<Mutex<HashMap<String, NetworkSnapshot>>>,
    is_running: Arc<Mutex<bool>>,
}

#[derive(Debug, Clone)]
struct NetworkSnapshot {
    interface_name: String,
    is_up: bool,
    bytes_sent: u64,
    bytes_received: u64,
    total_bytes_sent: u64,
    total_bytes_received: u64,
}

impl NetworkHandler {
    pub fn new(config: NetworkConfig) -> Self {
        Self {
            config,
            system: Arc::new(Mutex::new(System::new_all())),
            previous_interfaces: Arc::new(Mutex::new(HashMap::new())),
            is_running: Arc::new(Mutex::new(false)),
        }
    }

    #[cfg(windows)]
    async fn start_platform_specific(&self, sender: Sender<EventMessage>, handler_id: HandlerId) -> Result<()> {
        windows::start_network_monitoring(
            &self.config,
            &self.system,
            &self.previous_interfaces,
            &self.is_running,
            sender,
            handler_id,
        ).await
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    async fn start_platform_specific(&self, sender: Sender<EventMessage>, handler_id: HandlerId) -> Result<()> {
        unix::start_network_monitoring(
            &self.config,
            &self.system,
            &self.previous_interfaces,
            &self.is_running,
            sender,
            handler_id,
        ).await
    }

    #[cfg(target_os = "macos")]
    async fn start_platform_specific(&self, sender: Sender<EventMessage>, handler_id: HandlerId) -> Result<()> {
        macos::start_network_monitoring(
            &self.config,
            &self.system,
            &self.previous_interfaces,
            &self.is_running,
            sender,
            handler_id,
        ).await
    }

    pub fn emit_network_event(
        event_type: NetworkEventType,
        interface_name: Option<String>,
        local_addr: Option<String>,
        remote_addr: Option<String>,
        bytes_sent: Option<u64>,
        bytes_received: Option<u64>,
        sender: &Sender<EventMessage>,
        handler_id: &HandlerId,
    ) {
        let event_data = NetworkEventData {
            event_type,
            interface_name,
            local_addr,
            remote_addr,
            bytes_sent,
            bytes_received,
            timestamp: SystemTime::now(),
        };

        let event_message = EventMessage {
            data: EventData::Network(event_data),
            metadata: EventMetadata {
                id: uuid::Uuid::new_v4().as_u128() as EventId,
                handler_id: handler_id.clone(),
                timestamp: SystemTime::now(),
                source: "NetworkHandler".to_string(),
            },
        };

        if let Err(e) = sender.send(event_message) {
            log::error!("Failed to send network event: {}", e);
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

        log::info!("Starting network monitoring with native OS callbacks");
        self.start_platform_specific(sender, handler_id).await
    }

    pub async fn stop(&mut self) -> Result<()> {
        let mut is_running = self.is_running.lock().unwrap();
        *is_running = false;
        log::info!("Network monitoring stopped");
        Ok(())
    }
}