use crate::events::{EventData, PowerEventData, PowerEventType};
use crate::traits::{EventHandler, EventHandlerConfig, ThresholdConfig, IntervalConfig};
use crate::{EventBus, EventMessage, EventMetadata, HandlerId, Result, TellMeWhenError};
use crossbeam_channel::Sender;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};
use tokio::time::interval;

#[cfg(windows)]
use winapi::um::winbase::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};

#[cfg(target_os = "linux")]
use std::fs;

#[cfg(target_os = "macos")]
use core_foundation::base::TCFType;

#[derive(Debug, Clone)]
pub struct PowerConfig {
    pub base: EventHandlerConfig,
    pub battery_low_threshold: f32,
    pub monitor_battery: bool,
    pub monitor_power_source: bool,
    pub monitor_sleep_wake: bool,
}

impl Default for PowerConfig {
    fn default() -> Self {
        Self {
            base: EventHandlerConfig::default(),
            battery_low_threshold: 20.0, // 20%
            monitor_battery: true,
            monitor_power_source: true,
            monitor_sleep_wake: true,
        }
    }
}

impl ThresholdConfig for PowerConfig {
    fn set_threshold(&mut self, threshold: f32) {
        self.battery_low_threshold = threshold;
    }

    fn get_threshold(&self) -> f32 {
        self.battery_low_threshold
    }
}

impl IntervalConfig for PowerConfig {
    fn set_interval(&mut self, interval: Duration) {
        self.base.poll_interval = interval;
    }

    fn get_interval(&self) -> Duration {
        self.base.poll_interval
    }
}

#[derive(Debug, Clone)]
struct PowerSnapshot {
    battery_level: Option<f32>,
    is_charging: Option<bool>,
    power_source: Option<String>,
    is_battery_present: bool,
}

pub struct PowerHandler {
    config: PowerConfig,
    previous_state: Arc<Mutex<Option<PowerSnapshot>>>,
    pub event_sender: Option<Sender<EventMessage>>,
    is_running: bool,
    handler_id: HandlerId,
    monitor_task: Option<tokio::task::JoinHandle<()>>,
}

impl PowerHandler {
    pub fn new(handler_id: HandlerId) -> Self {
        Self {
            config: PowerConfig::default(),
            previous_state: Arc::new(Mutex::new(None)),
            event_sender: None,
            is_running: false,
            handler_id,
            monitor_task: None,
        }
    }

    pub fn with_config(handler_id: HandlerId, config: PowerConfig) -> Self {
        Self {
            config,
            previous_state: Arc::new(Mutex::new(None)),
            event_sender: None,
            is_running: false,
            handler_id,
            monitor_task: None,
        }
    }

    fn start_monitoring(&mut self) {
        let previous_state = self.previous_state.clone();
        let config = self.config.clone();
        let event_sender = self.event_sender.clone();
        let handler_id = self.handler_id.clone();

        let task = tokio::spawn(async move {
            let mut interval = interval(config.base.poll_interval);
            
            loop {
                interval.tick().await;
                
                if let Some(sender) = &event_sender {
                    Self::check_power_status(
                        &previous_state,
                        &config,
                        sender,
                        &handler_id,
                    ).await;
                }
            }
        });

        self.monitor_task = Some(task);
    }

    async fn check_power_status(
        previous_state: &Arc<Mutex<Option<PowerSnapshot>>>,
        config: &PowerConfig,
        sender: &Sender<EventMessage>,
        handler_id: &HandlerId,
    ) {
        let current_state = Self::get_power_status();
        let mut previous = previous_state.lock().unwrap();

        if let Some(current) = &current_state {
            // Check battery level changes
            if config.monitor_battery {
                if let Some(battery_level) = current.battery_level {
                    if battery_level <= config.battery_low_threshold {
                        Self::emit_power_event(
                            PowerEventType::BatteryLow,
                            Some(battery_level),
                            current.is_charging,
                            current.power_source.clone(),
                            sender,
                            handler_id,
                        );
                    }
                }

                // Check charging state changes
                if let Some(prev) = previous.as_ref() {
                    if let (Some(prev_charging), Some(curr_charging)) = (prev.is_charging, current.is_charging) {
                        if !prev_charging && curr_charging {
                            Self::emit_power_event(
                                PowerEventType::BatteryCharging,
                                current.battery_level,
                                Some(curr_charging),
                                current.power_source.clone(),
                                sender,
                                handler_id,
                            );
                        } else if prev_charging && !curr_charging {
                            Self::emit_power_event(
                                PowerEventType::BatteryDischarging,
                                current.battery_level,
                                Some(curr_charging),
                                current.power_source.clone(),
                                sender,
                                handler_id,
                            );
                        }
                    }
                }
            }

            // Check power source changes
            if config.monitor_power_source {
                if let Some(prev) = previous.as_ref() {
                    if prev.power_source != current.power_source {
                        Self::emit_power_event(
                            PowerEventType::PowerSourceChanged,
                            current.battery_level,
                            current.is_charging,
                            current.power_source.clone(),
                            sender,
                            handler_id,
                        );
                    }
                }
            }
        }

        *previous = current_state;
    }

    #[cfg(windows)]
    fn get_power_status() -> Option<PowerSnapshot> {
        unsafe {
            let mut status: SYSTEM_POWER_STATUS = std::mem::zeroed();
            if GetSystemPowerStatus(&mut status) != 0 {
                let battery_level = if status.BatteryLifePercent != 255 {
                    Some(status.BatteryLifePercent as f32)
                } else {
                    None
                };

                let is_charging = match status.ACLineStatus {
                    1 => Some(true),  // AC power
                    0 => Some(false), // Battery power
                    _ => None,        // Unknown
                };

                let power_source = match status.ACLineStatus {
                    1 => Some("AC".to_string()),
                    0 => Some("Battery".to_string()),
                    _ => Some("Unknown".to_string()),
                };

                let is_battery_present = status.BatteryFlag != 128; // 128 = no system battery

                Some(PowerSnapshot {
                    battery_level,
                    is_charging,
                    power_source,
                    is_battery_present,
                })
            } else {
                None
            }
        }
    }

    #[cfg(target_os = "linux")]
    fn get_power_status() -> Option<PowerSnapshot> {
        // Try to read from /sys/class/power_supply/
        let power_supply_path = "/sys/class/power_supply/";
        
        let mut battery_level = None;
        let mut is_charging = None;
        let mut power_source = None;
        let mut is_battery_present = false;

        if let Ok(entries) = fs::read_dir(power_supply_path) {
            for entry in entries.flatten() {
                let path = entry.path();
                let name = entry.file_name().to_string_lossy().to_string();

                if name.starts_with("BAT") {
                    is_battery_present = true;
                    
                    // Read battery capacity
                    if let Ok(capacity) = fs::read_to_string(path.join("capacity")) {
                        if let Ok(level) = capacity.trim().parse::<f32>() {
                            battery_level = Some(level);
                        }
                    }

                    // Read charging status
                    if let Ok(status) = fs::read_to_string(path.join("status")) {
                        let status = status.trim();
                        is_charging = match status {
                            "Charging" => Some(true),
                            "Discharging" | "Not charging" => Some(false),
                            _ => None,
                        };
                    }
                } else if name.starts_with("AC") || name.starts_with("ADP") {
                    // Read AC adapter status
                    if let Ok(online) = fs::read_to_string(path.join("online")) {
                        let online = online.trim() == "1";
                        power_source = Some(if online { "AC".to_string() } else { "Battery".to_string() });
                    }
                }
            }
        }

        Some(PowerSnapshot {
            battery_level,
            is_charging,
            power_source,
            is_battery_present,
        })
    }

    #[cfg(target_os = "macos")]
    fn get_power_status() -> Option<PowerSnapshot> {
        // This is a simplified implementation
        // In a real implementation, you'd use IOKit to get power information
        use core_foundation::dictionary::CFDictionary;
        use core_foundation::string::CFString;
        
        // Placeholder implementation - would need proper IOKit bindings
        Some(PowerSnapshot {
            battery_level: None,
            is_charging: None,
            power_source: Some("Unknown".to_string()),
            is_battery_present: false,
        })
    }

    #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
    fn get_power_status() -> Option<PowerSnapshot> {
        // Unsupported platform
        None
    }

    fn emit_power_event(
        event_type: PowerEventType,
        battery_level: Option<f32>,
        is_charging: Option<bool>,
        power_source: Option<String>,
        sender: &Sender<EventMessage>,
        handler_id: &HandlerId,
    ) {
        let event_data = PowerEventData {
            event_type,
            battery_level,
            is_charging,
            power_source,
            timestamp: SystemTime::now(),
        };

        let message = EventMessage {
            metadata: EventMetadata {
                id: 0, // Will be set by event bus
                handler_id: handler_id.clone(),
                timestamp: SystemTime::now(),
                source: "power".to_string(),
            },
            data: EventData::Power(event_data),
        };

        if let Err(e) = sender.send(message) {
            log::error!("Failed to send power event: {}", e);
        }
    }
}

#[async_trait::async_trait]
impl EventHandler for PowerHandler {
    type EventType = PowerEventData;
    type Config = PowerConfig;

    async fn start(&mut self, config: Self::Config) -> Result<()> {
        if self.is_running {
            return Ok(());
        }

        self.config = config;
        self.start_monitoring();
        self.is_running = true;

        log::info!("Power handler started with id: {}", self.handler_id);
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
        log::info!("Power handler stopped: {}", self.handler_id);
        Ok(())
    }

    fn is_running(&self) -> bool {
        self.is_running
    }

    fn name(&self) -> &'static str {
        "power"
    }
}