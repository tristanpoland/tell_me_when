use crate::events::{EventData, NetworkEventData, NetworkEventType};
use crate::traits::{EventHandler, EventHandlerConfig, ThresholdConfig, IntervalConfig};
use crate::{EventBus, EventMessage, EventMetadata, HandlerId, Result, TellMeWhenError};
use crossbeam_channel::Sender;
use sysinfo::System;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};
use tokio::time::interval;

#[derive(Debug, Clone)]
pub struct NetworkConfig {
    pub base: EventHandlerConfig,
    pub traffic_threshold: u64, // bytes per second
    pub monitor_interface_changes: bool,
    pub monitor_traffic: bool,
    pub interface_filters: Vec<String>,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            base: EventHandlerConfig::default(),
            traffic_threshold: 10_000_000, // 10MB/s
            monitor_interface_changes: true,
            monitor_traffic: true,
            interface_filters: Vec::new(),
        }
    }
}

impl ThresholdConfig for NetworkConfig {
    fn set_threshold(&mut self, threshold: f32) {
        self.traffic_threshold = threshold as u64;
    }

    fn get_threshold(&self) -> f32 {
        self.traffic_threshold as f32
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

#[derive(Debug, Clone)]
struct NetworkSnapshot {
    interface_name: String,
    is_up: bool,
    bytes_sent: u64,
    bytes_received: u64,
    total_bytes_sent: u64,
    total_bytes_received: u64,
}

pub struct NetworkHandler {
    config: NetworkConfig,
    system: Arc<Mutex<System>>,
    previous_networks: Arc<Mutex<HashMap<String, NetworkSnapshot>>>,
    pub event_sender: Option<Sender<EventMessage>>,
    is_running: bool,
    handler_id: HandlerId,
    monitor_task: Option<tokio::task::JoinHandle<()>>,
}

impl NetworkHandler {
    pub fn new(handler_id: HandlerId) -> Self {
        Self {
            config: NetworkConfig::default(),
            system: Arc::new(Mutex::new(System::new_all())),
            previous_networks: Arc::new(Mutex::new(HashMap::new())),
            event_sender: None,
            is_running: false,
            handler_id,
            monitor_task: None,
        }
    }

    pub fn with_config(handler_id: HandlerId, config: NetworkConfig) -> Self {
        Self {
            config,
            system: Arc::new(Mutex::new(System::new_all())),
            previous_networks: Arc::new(Mutex::new(HashMap::new())),
            event_sender: None,
            is_running: false,
            handler_id,
            monitor_task: None,
        }
    }

    fn start_monitoring(&mut self) {
        let system = self.system.clone();
        let previous_networks = self.previous_networks.clone();
        let config = self.config.clone();
        let event_sender = self.event_sender.clone();
        let handler_id = self.handler_id.clone();

        let task = tokio::spawn(async move {
            let mut interval = interval(config.base.poll_interval);
            
            loop {
                interval.tick().await;
                
                if let Some(sender) = &event_sender {
                    Self::check_network_changes(
                        &system,
                        &previous_networks,
                        &config,
                        sender,
                        &handler_id,
                    ).await;
                }
            }
        });

        self.monitor_task = Some(task);
    }

    async fn check_network_changes(
        system: &Arc<Mutex<System>>,
        previous_networks: &Arc<Mutex<HashMap<String, NetworkSnapshot>>>,
        config: &NetworkConfig,
        sender: &Sender<EventMessage>,
        handler_id: &HandlerId,
    ) {
        // Network monitoring would require platform-specific implementation
        // For newer sysinfo versions, this API has changed
        let current_networks: HashMap<String, NetworkSnapshot> = HashMap::new();

        let mut previous = previous_networks.lock().unwrap();

        // Network monitoring functionality would be implemented here
        // This requires platform-specific network interface detection

        *previous = current_networks;
    }

    fn should_monitor_interface(interface_name: &str, config: &NetworkConfig) -> bool {
        if config.interface_filters.is_empty() {
            return true;
        }

        config.interface_filters.iter().any(|filter| {
            if filter.contains('*') {
                let regex_pattern = filter.replace("*", ".*");
                if let Ok(regex) = regex::Regex::new(&regex_pattern) {
                    return regex.is_match(interface_name);
                }
            }
            interface_name.contains(filter)
        })
    }

    fn emit_network_event(
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

        let message = EventMessage {
            metadata: EventMetadata {
                id: 0, // Will be set by event bus
                handler_id: handler_id.clone(),
                timestamp: SystemTime::now(),
                source: "network".to_string(),
            },
            data: EventData::Network(event_data),
        };

        if let Err(e) = sender.send(message) {
            log::error!("Failed to send network event: {}", e);
        }
    }
}

#[async_trait::async_trait]
impl EventHandler for NetworkHandler {
    type EventType = NetworkEventData;
    type Config = NetworkConfig;

    async fn start(&mut self, config: Self::Config) -> Result<()> {
        if self.is_running {
            return Ok(());
        }

        self.config = config;
        
        // Initialize system information
        // Network monitoring setup would go here

        self.start_monitoring();
        self.is_running = true;

        log::info!("Network handler started with id: {}", self.handler_id);
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
        log::info!("Network handler stopped: {}", self.handler_id);
        Ok(())
    }

    fn is_running(&self) -> bool {
        self.is_running
    }

    fn name(&self) -> &'static str {
        "network"
    }
}