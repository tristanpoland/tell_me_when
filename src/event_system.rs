use crate::events::*;
use crate::handlers::*;
use crate::traits::*;
use crate::{EventBus, EventId, EventMessage, HandlerId, Result, TellMeWhenError};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct EventSystem {
    event_bus: Arc<EventBus>,
    fs_handler: Option<FileSystemHandler>,
    process_handler: Option<ProcessHandler>,
    system_handler: Option<SystemHandler>,
    network_handler: Option<NetworkHandler>,
    power_handler: Option<PowerHandler>,
    is_running: bool,
}

impl EventSystem {
    pub fn new() -> Self {
        let event_bus = Arc::new(EventBus::new());
        
        Self {
            event_bus,
            fs_handler: None,
            process_handler: None,
            system_handler: None,
            network_handler: None,
            power_handler: None,
            is_running: false,
        }
    }

    pub async fn start(&mut self) -> Result<()> {
        if self.is_running {
            return Ok(());
        }

        self.event_bus.start_processing().await;
        self.is_running = true;
        
        log::info!("EventSystem started");
        Ok(())
    }

    pub async fn stop(&mut self) -> Result<()> {
        if !self.is_running {
            return Ok(());
        }

        // Stop all handlers
        if let Some(ref mut handler) = self.fs_handler {
            handler.stop().await?;
        }
        if let Some(ref mut handler) = self.process_handler {
            handler.stop().await?;
        }
        if let Some(ref mut handler) = self.system_handler {
            handler.stop().await?;
        }
        if let Some(ref mut handler) = self.network_handler {
            handler.stop().await?;
        }
        if let Some(ref mut handler) = self.power_handler {
            handler.stop().await?;
        }

        self.is_running = false;
        log::info!("EventSystem stopped");
        Ok(())
    }

    // Filesystem event methods
    pub async fn on_fs_event<F, P>(&mut self, path: P, callback: F) -> Result<EventId>
    where
        F: Fn(FsEventData) + Send + Sync + 'static,
        P: AsRef<Path>,
    {
        self.ensure_fs_handler().await?;
        
        if let Some(ref mut handler) = self.fs_handler {
            handler.watch_path(path).await?;
        }

        let event_id = self.event_bus.subscribe(move |message| {
            if let EventData::FileSystem(fs_data) = message.data {
                callback(fs_data);
            }
        }).await;

        Ok(event_id)
    }

    pub async fn on_fs_created<F, P>(&mut self, path: P, callback: F) -> Result<EventId>
    where
        F: Fn(FsEventData) + Send + Sync + 'static,
        P: AsRef<Path>,
    {
        self.on_fs_event_filtered(path, FsEventType::Created, callback).await
    }

    pub async fn on_fs_modified<F, P>(&mut self, path: P, callback: F) -> Result<EventId>
    where
        F: Fn(FsEventData) + Send + Sync + 'static,
        P: AsRef<Path>,
    {
        self.on_fs_event_filtered(path, FsEventType::Modified, callback).await
    }

    pub async fn on_fs_deleted<F, P>(&mut self, path: P, callback: F) -> Result<EventId>
    where
        F: Fn(FsEventData) + Send + Sync + 'static,
        P: AsRef<Path>,
    {
        self.on_fs_event_filtered(path, FsEventType::Deleted, callback).await
    }

    async fn on_fs_event_filtered<F, P>(&mut self, path: P, event_type: FsEventType, callback: F) -> Result<EventId>
    where
        F: Fn(FsEventData) + Send + Sync + 'static,
        P: AsRef<Path>,
    {
        self.ensure_fs_handler().await?;
        
        if let Some(ref mut handler) = self.fs_handler {
            handler.watch_path(path).await?;
        }

        let event_id = self.event_bus.subscribe(move |message| {
            if let EventData::FileSystem(fs_data) = message.data {
                if std::mem::discriminant(&fs_data.event_type) == std::mem::discriminant(&event_type) {
                    callback(fs_data);
                }
            }
        }).await;

        Ok(event_id)
    }

    // Process event methods
    pub async fn on_process_event<F>(&mut self, callback: F) -> Result<EventId>
    where
        F: Fn(ProcessEventData) + Send + Sync + 'static,
    {
        self.ensure_process_handler().await?;

        let event_id = self.event_bus.subscribe(move |message| {
            if let EventData::Process(process_data) = message.data {
                callback(process_data);
            }
        }).await;

        Ok(event_id)
    }

    pub async fn on_process_started<F>(&mut self, callback: F) -> Result<EventId>
    where
        F: Fn(ProcessEventData) + Send + Sync + 'static,
    {
        self.on_process_event_filtered(ProcessEventType::Started, callback).await
    }

    pub async fn on_process_terminated<F>(&mut self, callback: F) -> Result<EventId>
    where
        F: Fn(ProcessEventData) + Send + Sync + 'static,
    {
        self.on_process_event_filtered(ProcessEventType::Terminated, callback).await
    }

    async fn on_process_event_filtered<F>(&mut self, event_type: ProcessEventType, callback: F) -> Result<EventId>
    where
        F: Fn(ProcessEventData) + Send + Sync + 'static,
    {
        self.ensure_process_handler().await?;

        let event_id = self.event_bus.subscribe(move |message| {
            if let EventData::Process(process_data) = message.data {
                if process_data.event_type == event_type {
                    callback(process_data);
                }
            }
        }).await;

        Ok(event_id)
    }

    // System event methods
    pub async fn on_system_event<F>(&mut self, callback: F) -> Result<EventId>
    where
        F: Fn(SystemEventData) + Send + Sync + 'static,
    {
        self.ensure_system_handler().await?;

        let event_id = self.event_bus.subscribe(move |message| {
            if let EventData::System(system_data) = message.data {
                callback(system_data);
            }
        }).await;

        Ok(event_id)
    }

    pub async fn on_cpu_usage_high<F>(&mut self, threshold: f32, callback: F) -> Result<EventId>
    where
        F: Fn(SystemEventData) + Send + Sync + 'static,
    {
        self.ensure_system_handler().await?;

        let event_id = self.event_bus.subscribe(move |message| {
            if let EventData::System(system_data) = message.data {
                if system_data.event_type == SystemEventType::CpuUsageHigh {
                    if let Some(cpu_usage) = system_data.cpu_usage {
                        if cpu_usage >= threshold {
                            callback(system_data);
                        }
                    }
                }
            }
        }).await;

        Ok(event_id)
    }

    pub async fn on_memory_usage_high<F>(&mut self, threshold: f32, callback: F) -> Result<EventId>
    where
        F: Fn(SystemEventData) + Send + Sync + 'static,
    {
        self.ensure_system_handler().await?;

        let event_id = self.event_bus.subscribe(move |message| {
            if let EventData::System(system_data) = message.data {
                if system_data.event_type == SystemEventType::MemoryUsageHigh {
                    if let Some(memory_usage) = system_data.memory_usage {
                        if memory_usage >= threshold {
                            callback(system_data);
                        }
                    }
                }
            }
        }).await;

        Ok(event_id)
    }

    // Network event methods
    pub async fn on_network_event<F>(&mut self, callback: F) -> Result<EventId>
    where
        F: Fn(NetworkEventData) + Send + Sync + 'static,
    {
        self.ensure_network_handler().await?;

        let event_id = self.event_bus.subscribe(move |message| {
            if let EventData::Network(network_data) = message.data {
                callback(network_data);
            }
        }).await;

        Ok(event_id)
    }

    // Power event methods
    pub async fn on_power_event<F>(&mut self, callback: F) -> Result<EventId>
    where
        F: Fn(PowerEventData) + Send + Sync + 'static,
    {
        self.ensure_power_handler().await?;

        let event_id = self.event_bus.subscribe(move |message| {
            if let EventData::Power(power_data) = message.data {
                callback(power_data);
            }
        }).await;

        Ok(event_id)
    }

    pub async fn on_battery_low<F>(&mut self, threshold: f32, callback: F) -> Result<EventId>
    where
        F: Fn(PowerEventData) + Send + Sync + 'static,
    {
        self.ensure_power_handler().await?;

        let event_id = self.event_bus.subscribe(move |message| {
            if let EventData::Power(power_data) = message.data {
                if power_data.event_type == PowerEventType::BatteryLow {
                    if let Some(battery_level) = power_data.battery_level {
                        if battery_level <= threshold {
                            callback(power_data);
                        }
                    }
                }
            }
        }).await;

        Ok(event_id)
    }

    // Utility methods
    pub async fn unsubscribe(&self, event_id: EventId) -> bool {
        self.event_bus.unsubscribe(event_id).await
    }

    pub fn is_running(&self) -> bool {
        self.is_running
    }

    // Handler initialization methods
    async fn ensure_fs_handler(&mut self) -> Result<()> {
        if self.fs_handler.is_none() {
            let mut handler = FileSystemHandler::new("filesystem".to_string());
            handler.event_sender = Some(self.event_bus.sender());
            handler.start(Default::default()).await?;
            self.fs_handler = Some(handler);
        }
        Ok(())
    }

    async fn ensure_process_handler(&mut self) -> Result<()> {
        if self.process_handler.is_none() {
            let mut handler = ProcessHandler::new("process".to_string());
            handler.event_sender = Some(self.event_bus.sender());
            handler.start(Default::default()).await?;
            self.process_handler = Some(handler);
        }
        Ok(())
    }

    async fn ensure_system_handler(&mut self) -> Result<()> {
        if self.system_handler.is_none() {
            let mut handler = SystemHandler::new("system".to_string());
            handler.event_sender = Some(self.event_bus.sender());
            handler.start(Default::default()).await?;
            self.system_handler = Some(handler);
        }
        Ok(())
    }

    async fn ensure_network_handler(&mut self) -> Result<()> {
        if self.network_handler.is_none() {
            let mut handler = NetworkHandler::new("network".to_string());
            handler.event_sender = Some(self.event_bus.sender());
            handler.start(Default::default()).await?;
            self.network_handler = Some(handler);
        }
        Ok(())
    }

    async fn ensure_power_handler(&mut self) -> Result<()> {
        if self.power_handler.is_none() {
            let mut handler = PowerHandler::new("power".to_string());
            handler.event_sender = Some(self.event_bus.sender());
            handler.start(Default::default()).await?;
            self.power_handler = Some(handler);
        }
        Ok(())
    }
}

impl Default for EventSystem {
    fn default() -> Self {
        Self::new()
    }
}