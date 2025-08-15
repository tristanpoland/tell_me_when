use crate::events::{EventData, ProcessEventData, ProcessEventType};
use crate::traits::{EventHandler, EventHandlerConfig, ThresholdConfig, IntervalConfig};
use crate::{EventBus, EventMessage, EventMetadata, HandlerId, Result, TellMeWhenError};
use crossbeam_channel::Sender;
use sysinfo::{System, Pid, Process};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};
use tokio::time::interval;

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
    previous_processes: Arc<Mutex<HashMap<Pid, ProcessSnapshot>>>,
    pub event_sender: Option<Sender<EventMessage>>,
    is_running: bool,
    handler_id: HandlerId,
    monitor_task: Option<tokio::task::JoinHandle<()>>,
}

#[derive(Debug, Clone)]
struct ProcessSnapshot {
    pid: Pid,
    name: String,
    cpu_usage: f32,
    memory: u64,
    status: String,
}

impl ProcessHandler {
    pub fn new(handler_id: HandlerId) -> Self {
        Self {
            config: ProcessConfig::default(),
            system: Arc::new(Mutex::new(System::new_all())),
            previous_processes: Arc::new(Mutex::new(HashMap::new())),
            event_sender: None,
            is_running: false,
            handler_id,
            monitor_task: None,
        }
    }

    pub fn with_config(handler_id: HandlerId, config: ProcessConfig) -> Self {
        Self {
            config,
            system: Arc::new(Mutex::new(System::new_all())),
            previous_processes: Arc::new(Mutex::new(HashMap::new())),
            event_sender: None,
            is_running: false,
            handler_id,
            monitor_task: None,
        }
    }

    fn start_monitoring(&mut self) {
        let system = self.system.clone();
        let previous_processes = self.previous_processes.clone();
        let config = self.config.clone();
        let event_sender = self.event_sender.clone();
        let handler_id = self.handler_id.clone();

        let task = tokio::spawn(async move {
            let mut interval = interval(config.base.poll_interval);
            
            loop {
                interval.tick().await;
                
                if let Some(sender) = &event_sender {
                    Self::check_processes(
                        &system,
                        &previous_processes,
                        &config,
                        sender,
                        &handler_id,
                    ).await;
                }
            }
        });

        self.monitor_task = Some(task);
    }

    async fn check_processes(
        system: &Arc<Mutex<System>>,
        previous_processes: &Arc<Mutex<HashMap<Pid, ProcessSnapshot>>>,
        config: &ProcessConfig,
        sender: &Sender<EventMessage>,
        handler_id: &HandlerId,
    ) {
        let current_processes = {
            let mut sys = system.lock().unwrap();
            sys.refresh_processes();
            
            let mut processes = HashMap::new();
            for (pid, process) in sys.processes() {
                if Self::should_monitor_process(process, config) {
                    let snapshot = ProcessSnapshot {
                        pid: *pid,
                        name: process.name().to_string(),
                        cpu_usage: process.cpu_usage(),
                        memory: process.memory(),
                        status: format!("{:?}", process.status()),
                    };
                    processes.insert(*pid, snapshot);
                }
            }
            processes
        };

        let mut previous = previous_processes.lock().unwrap();
        
        // Check for new processes
        if config.monitor_new_processes {
            for (pid, process) in &current_processes {
                if !previous.contains_key(pid) {
                    Self::emit_process_event(
                        ProcessEventType::Started,
                        process,
                        sender,
                        handler_id,
                    );
                }
            }
        }

        // Check for terminated processes
        if config.monitor_terminated_processes {
            for (pid, process) in previous.iter() {
                if !current_processes.contains_key(pid) {
                    Self::emit_process_event(
                        ProcessEventType::Terminated,
                        process,
                        sender,
                        handler_id,
                    );
                }
            }
        }

        // Check for threshold violations
        for (pid, process) in &current_processes {
            if process.cpu_usage >= config.cpu_threshold {
                Self::emit_process_event(
                    ProcessEventType::CpuUsageHigh,
                    process,
                    sender,
                    handler_id,
                );
            }

            if process.memory >= config.memory_threshold {
                Self::emit_process_event(
                    ProcessEventType::MemoryUsageHigh,
                    process,
                    sender,
                    handler_id,
                );
            }

            // Check for status changes
            if let Some(prev_process) = previous.get(pid) {
                if prev_process.status != process.status {
                    Self::emit_process_event(
                        ProcessEventType::StatusChanged,
                        process,
                        sender,
                        handler_id,
                    );
                }
            }
        }

        *previous = current_processes;
    }

    fn should_monitor_process(process: &Process, config: &ProcessConfig) -> bool {
        if config.process_name_filters.is_empty() {
            return true;
        }

        let process_name = process.name();
        config.process_name_filters.iter().any(|filter| {
            if filter.contains('*') {
                let regex_pattern = filter.replace("*", ".*");
                if let Ok(regex) = regex::Regex::new(&regex_pattern) {
                    return regex.is_match(&process_name);
                }
            }
            process_name.contains(filter)
        })
    }

    fn emit_process_event(
        event_type: ProcessEventType,
        process: &ProcessSnapshot,
        sender: &Sender<EventMessage>,
        handler_id: &HandlerId,
    ) {
        let event_data = ProcessEventData {
            event_type,
            pid: process.pid.as_u32(),
            name: process.name.clone(),
            cpu_usage: Some(process.cpu_usage),
            memory_usage: Some(process.memory),
            timestamp: SystemTime::now(),
        };

        let message = EventMessage {
            metadata: EventMetadata {
                id: 0, // Will be set by event bus
                handler_id: handler_id.clone(),
                timestamp: SystemTime::now(),
                source: "process".to_string(),
            },
            data: EventData::Process(event_data),
        };

        if let Err(e) = sender.send(message) {
            log::error!("Failed to send process event: {}", e);
        }
    }
}

#[async_trait::async_trait]
impl EventHandler for ProcessHandler {
    type EventType = ProcessEventData;
    type Config = ProcessConfig;

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

        log::info!("Process handler started with id: {}", self.handler_id);
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
        log::info!("Process handler stopped: {}", self.handler_id);
        Ok(())
    }

    fn is_running(&self) -> bool {
        self.is_running
    }

    fn name(&self) -> &'static str {
        "process"
    }
}