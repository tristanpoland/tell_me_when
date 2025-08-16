use super::{ProcessConfig, ProcessSnapshot, ProcessHandler};
use crate::events::{ProcessEventType};
use crate::{EventMessage, HandlerId, Result, TellMeWhenError};
use crossbeam_channel::Sender;
use sysinfo::{System};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::SystemTime;
use tokio::task;

pub async fn start_process_monitoring(
    config: &ProcessConfig,
    system: &Arc<Mutex<System>>,
    previous_processes: &Arc<Mutex<HashMap<u32, ProcessSnapshot>>>,
    is_running: &Arc<Mutex<bool>>,
    sender: Sender<EventMessage>,
    handler_id: HandlerId,
) -> Result<()> {
    let config = config.clone();
    let is_running = Arc::clone(is_running);
    let sender_clone = sender.clone();
    let handler_id_clone = handler_id.clone();

    // Start WMI event notifications for process creation
    if config.monitor_new_processes {
        let creation_sender = sender_clone.clone();
        let creation_handler_id = handler_id_clone.clone();
        let creation_is_running = Arc::clone(&is_running);
        
        task::spawn_blocking(move || {
            if let Err(e) = monitor_process_creation_events(creation_sender, creation_handler_id, creation_is_running) {
                log::error!("Process creation monitoring failed: {}", e);
            }
        });
    }

    // Start WMI event notifications for process termination
    if config.monitor_terminated_processes {
        let termination_sender = sender_clone.clone();
        let termination_handler_id = handler_id_clone.clone();
        let termination_is_running = Arc::clone(&is_running);
        
        task::spawn_blocking(move || {
            if let Err(e) = monitor_process_termination_events(termination_sender, termination_handler_id, termination_is_running) {
                log::error!("Process termination monitoring failed: {}", e);
            }
        });
    }

    // For high resource usage, we'll use a different approach with periodic checks
    // but only when thresholds are crossed
    let resource_config = config.clone();
    let resource_system = Arc::clone(system);
    let resource_previous = Arc::clone(previous_processes);
    let resource_sender = sender;
    let resource_handler_id = handler_id;
    let resource_is_running = Arc::clone(&is_running);

    task::spawn_blocking(move || {
        monitor_resource_thresholds(resource_config, resource_system, resource_previous, resource_sender, resource_handler_id, resource_is_running);
    });

    Ok(())
}

fn monitor_process_creation_events(
    sender: Sender<EventMessage>,
    handler_id: HandlerId,
    is_running: Arc<Mutex<bool>>,
) -> Result<()> {
    use wmi::{WMIConnection, COMLibrary, Variant};
    use std::collections::HashMap;
    
    log::info!("Starting Windows process creation event monitoring via WMI event notifications");

    let com_lib = COMLibrary::new().map_err(|e| {
        TellMeWhenError::System(format!("Failed to initialize COM library: {}", e))
    })?;
    
    let wmi_con = WMIConnection::new(com_lib).map_err(|e| {
        TellMeWhenError::System(format!("Failed to create WMI connection: {}", e))
    })?;

    // Use WMI raw notification for process start events
    let query = "SELECT ProcessID, ProcessName FROM Win32_ProcessStartTrace";
    
    while *is_running.lock().unwrap() {
        match wmi_con.raw_notification::<HashMap<String, Variant>>(query) {
            Ok(iterator) => {
                for event_result in iterator {
                    if !*is_running.lock().unwrap() {
                        break;
                    }
                    
                    match event_result {
                        Ok(event) => {
                            if let (Some(pid_value), Some(name_value)) = 
                                (event.get("ProcessID"), event.get("ProcessName")) {
                                
                                // Extract values based on WMI variant type
                                let pid = extract_u32_from_variant(pid_value)?;
                                let name = extract_string_from_variant(name_value)?;
                                
                                log::debug!("WMI Process creation event: {} (PID: {})", name, pid);
                                
                                ProcessHandler::emit_process_event(
                                    ProcessEventType::Started,
                                    pid,
                                    name,
                                    None,
                                    None,
                                    &sender,
                                    &handler_id,
                                );
                            }
                        }
                        Err(e) => {
                            log::warn!("WMI event error: {}", e);
                        }
                    }
                }
            }
            Err(e) => {
                log::error!("WMI notification query failed: {}", e);
                // Wait a bit before retrying to avoid tight loop
                std::thread::sleep(std::time::Duration::from_secs(5));
            }
        }
    }

    Ok(())
}

fn monitor_process_termination_events(
    sender: Sender<EventMessage>,
    handler_id: HandlerId,
    is_running: Arc<Mutex<bool>>,
) -> Result<()> {
    use wmi::{WMIConnection, COMLibrary, Variant};
    use std::collections::HashMap;
    
    log::info!("Starting Windows process termination event monitoring via WMI event notifications");

    let com_lib = COMLibrary::new().map_err(|e| {
        TellMeWhenError::System(format!("Failed to initialize COM library: {}", e))
    })?;
    
    let wmi_con = WMIConnection::new(com_lib).map_err(|e| {
        TellMeWhenError::System(format!("Failed to create WMI connection: {}", e))
    })?;

    // Use WMI raw notification for process stop events
    let query = "SELECT ProcessID, ProcessName FROM Win32_ProcessStopTrace";
    
    while *is_running.lock().unwrap() {
        match wmi_con.raw_notification::<HashMap<String, Variant>>(query) {
            Ok(iterator) => {
                for event_result in iterator {
                    if !*is_running.lock().unwrap() {
                        break;
                    }
                    
                    match event_result {
                        Ok(event) => {
                            if let (Some(pid_value), Some(name_value)) = 
                                (event.get("ProcessID"), event.get("ProcessName")) {
                                
                                // Extract values based on WMI variant type
                                let pid = extract_u32_from_variant(pid_value)?;
                                let name = extract_string_from_variant(name_value)?;
                                
                                log::debug!("WMI Process termination event: {} (PID: {})", name, pid);
                                
                                ProcessHandler::emit_process_event(
                                    ProcessEventType::Terminated,
                                    pid,
                                    name,
                                    None,
                                    None,
                                    &sender,
                                    &handler_id,
                                );
                            }
                        }
                        Err(e) => {
                            log::warn!("WMI event error: {}", e);
                        }
                    }
                }
            }
            Err(e) => {
                log::error!("WMI notification query failed: {}", e);
                // Wait a bit before retrying to avoid tight loop
                std::thread::sleep(std::time::Duration::from_secs(5));
            }
        }
    }

    Ok(())
}

fn extract_u32_from_variant(variant: &wmi::Variant) -> Result<u32> {
    use wmi::Variant;
    match variant {
        Variant::UI4(val) => Ok(*val),
        Variant::I4(val) => Ok(*val as u32),
        _ => Err(TellMeWhenError::System("Invalid variant type for PID".to_string())),
    }
}

fn extract_string_from_variant(variant: &wmi::Variant) -> Result<String> {
    use wmi::Variant;
    match variant {
        Variant::String(val) => Ok(val.clone()),
        _ => Err(TellMeWhenError::System("Invalid variant type for process name".to_string())),
    }
}

fn monitor_resource_thresholds(
    config: ProcessConfig,
    system: Arc<Mutex<System>>,
    previous_processes: Arc<Mutex<HashMap<u32, ProcessSnapshot>>>,
    sender: Sender<EventMessage>,
    handler_id: HandlerId,
    is_running: Arc<Mutex<bool>>,
) {
    log::info!("Starting resource threshold monitoring (minimal polling for thresholds only)");
    
    // This is the only part that uses minimal polling - just for resource thresholds
    // Process creation/termination use pure WMI event callbacks above
    while *is_running.lock().unwrap() {
        {
            let mut sys = system.lock().unwrap();
            sys.refresh_processes();
            
            for (pid, process) in sys.processes() {
                let pid_u32 = pid.as_u32();
                let cpu_usage = process.cpu_usage();
                let memory_usage = process.memory();
                let name = process.name().to_string();
                
                // Only emit events when thresholds are crossed
                if cpu_usage > config.cpu_threshold {
                    ProcessHandler::emit_process_event(
                        ProcessEventType::CpuUsageHigh,
                        pid_u32,
                        name.clone(),
                        Some(cpu_usage),
                        Some(memory_usage),
                        &sender,
                        &handler_id,
                    );
                }
                
                if memory_usage > config.memory_threshold {
                    ProcessHandler::emit_process_event(
                        ProcessEventType::MemoryUsageHigh,
                        pid_u32,
                        name.clone(),
                        Some(cpu_usage),
                        Some(memory_usage),
                        &sender,
                        &handler_id,
                    );
                }
            }
        }
        
        // Check thresholds less frequently since this is the only polling part
        std::thread::sleep(std::time::Duration::from_secs(5));
    }
}