use super::{ProcessConfig, ProcessSnapshot, ProcessHandler};
use crate::events::{ProcessEventType};
use crate::{EventMessage, HandlerId, Result, TellMeWhenError};
use crossbeam_channel::Sender;
use sysinfo::{System, Pid, Process};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::SystemTime;
use tokio::task;
use winapi::um::winnt::HANDLE;
use winapi::um::synchapi::{CreateEventW, WaitForSingleObject};
use winapi::um::handleapi::{CloseHandle, INVALID_HANDLE_VALUE};
use winapi::um::winbase::{WAIT_OBJECT_0, INFINITE};
use winapi::shared::winerror::ERROR_SUCCESS;
use std::ptr;

pub async fn start_process_monitoring(
    config: &ProcessConfig,
    system: &Arc<Mutex<System>>,
    previous_processes: &Arc<Mutex<HashMap<u32, ProcessSnapshot>>>,
    is_running: &Arc<Mutex<bool>>,
    sender: Sender<EventMessage>,
    handler_id: HandlerId,
) -> Result<()> {
    let config = config.clone();
    let system = Arc::clone(system);
    let previous_processes = Arc::clone(previous_processes);
    let is_running = Arc::clone(is_running);

    // Use WMI for real-time process monitoring
    task::spawn_blocking(move || {
        if let Err(e) = start_wmi_process_monitoring(&config, &system, &previous_processes, &is_running, sender, handler_id) {
            log::error!("WMI process monitoring failed: {}", e);
        }
    }).await.map_err(|e| TellMeWhenError::System(format!("Process monitoring task failed: {}", e)))?;

    Ok(())
}

fn start_wmi_process_monitoring(
    config: &ProcessConfig,
    system: &Arc<Mutex<System>>,
    previous_processes: &Arc<Mutex<HashMap<u32, ProcessSnapshot>>>,
    is_running: &Arc<Mutex<bool>>,
    sender: Sender<EventMessage>,
    handler_id: HandlerId,
) -> Result<()> {
    use wmi::{WMIConnection, COMLibrary, Variant};
    use std::collections::HashMap as WmiHashMap;

    log::info!("Starting WMI process event monitoring");

    let wmi_con = WMIConnection::new(COMLibrary::new().map_err(|e| {
        TellMeWhenError::System(format!("Failed to initialize COM library: {}", e))
    })?)
    .map_err(|e| TellMeWhenError::System(format!("Failed to create WMI connection: {}", e)))?;

    // Monitor process creation events
    if config.monitor_new_processes {
        let creation_sender = sender.clone();
        let creation_handler_id = handler_id.clone();
        let creation_is_running = Arc::clone(is_running);

        std::thread::spawn(move || {
            if let Err(e) = monitor_process_creation(&wmi_con, creation_sender, creation_handler_id, creation_is_running) {
                log::error!("Process creation monitoring failed: {}", e);
            }
        });
    }

    // Monitor process termination events
    if config.monitor_terminated_processes {
        let termination_sender = sender.clone();
        let termination_handler_id = handler_id.clone();
        let termination_is_running = Arc::clone(is_running);

        std::thread::spawn(move || {
            if let Err(e) = monitor_process_termination(&wmi_con, termination_sender, termination_handler_id, termination_is_running) {
                log::error!("Process termination monitoring failed: {}", e);
            }
        });
    }

    // Monitor for high resource usage with polling (no native event for this)
    let resource_config = config.clone();
    let resource_system = Arc::clone(system);
    let resource_previous = Arc::clone(previous_processes);
    let resource_sender = sender;
    let resource_handler_id = handler_id;
    let resource_is_running = Arc::clone(is_running);

    std::thread::spawn(move || {
        monitor_resource_usage(resource_config, resource_system, resource_previous, resource_sender, resource_handler_id, resource_is_running);
    });

    Ok(())
}

fn monitor_process_creation(
    wmi_con: &wmi::WMIConnection,
    sender: Sender<EventMessage>,
    handler_id: HandlerId,
    is_running: Arc<Mutex<bool>>,
) -> Result<()> {
    use wmi::Variant;
    
    log::info!("Starting process creation event monitoring via WMI");

    // WMI event query for process creation
    let query = "SELECT * FROM Win32_ProcessStartTrace";
    
    while *is_running.lock().unwrap() {
        match wmi_con.raw_query(query) {
            Ok(results) => {
                for result in results {
                    if !*is_running.lock().unwrap() {
                        break;
                    }

                    if let Ok(process_data) = result {
                        if let (Some(Variant::UI4(pid)), Some(Variant::String(name))) = 
                            (process_data.get("ProcessID"), process_data.get("ProcessName")) {
                            
                            log::debug!("Process created: {} (PID: {})", name, pid);
                            
                            ProcessHandler::emit_process_event(
                                ProcessEventType::Started,
                                pid,
                                name.clone(),
                                None,
                                None,
                                &sender,
                                &handler_id,
                            );
                        }
                    }
                }
            }
            Err(e) => {
                log::warn!("WMI query failed, retrying: {}", e);
                std::thread::sleep(std::time::Duration::from_secs(5));
            }
        }
        
        std::thread::sleep(std::time::Duration::from_millis(100));
    }

    Ok(())
}

fn monitor_process_termination(
    wmi_con: &wmi::WMIConnection,
    sender: Sender<EventMessage>,
    handler_id: HandlerId,
    is_running: Arc<Mutex<bool>>,
) -> Result<()> {
    use wmi::Variant;
    
    log::info!("Starting process termination event monitoring via WMI");

    // WMI event query for process termination
    let query = "SELECT * FROM Win32_ProcessStopTrace";
    
    while *is_running.lock().unwrap() {
        match wmi_con.raw_query(query) {
            Ok(results) => {
                for result in results {
                    if !*is_running.lock().unwrap() {
                        break;
                    }

                    if let Ok(process_data) = result {
                        if let (Some(Variant::UI4(pid)), Some(Variant::String(name))) = 
                            (process_data.get("ProcessID"), process_data.get("ProcessName")) {
                            
                            log::debug!("Process terminated: {} (PID: {})", name, pid);
                            
                            ProcessHandler::emit_process_event(
                                ProcessEventType::Terminated,
                                pid,
                                name.clone(),
                                None,
                                None,
                                &sender,
                                &handler_id,
                            );
                        }
                    }
                }
            }
            Err(e) => {
                log::warn!("WMI query failed, retrying: {}", e);
                std::thread::sleep(std::time::Duration::from_secs(5));
            }
        }
        
        std::thread::sleep(std::time::Duration::from_millis(100));
    }

    Ok(())
}

fn monitor_resource_usage(
    config: ProcessConfig,
    system: Arc<Mutex<System>>,
    previous_processes: Arc<Mutex<HashMap<u32, ProcessSnapshot>>>,
    sender: Sender<EventMessage>,
    handler_id: HandlerId,
    is_running: Arc<Mutex<bool>>,
) {
    log::info!("Starting resource usage monitoring");
    
    while *is_running.lock().unwrap() {
        {
            let mut sys = system.lock().unwrap();
            sys.refresh_processes();
            
            let mut prev_processes = previous_processes.lock().unwrap();
            let mut current_pids = std::collections::HashSet::new();
            
            for (pid, process) in sys.processes() {
                let pid_u32 = pid.as_u32();
                current_pids.insert(pid_u32);
                
                let cpu_usage = process.cpu_usage();
                let memory_usage = process.memory();
                let name = process.name().to_string();
                
                // Check CPU threshold
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
                
                // Check memory threshold
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
                
                // Update process snapshot
                prev_processes.insert(pid_u32, ProcessSnapshot {
                    pid: pid_u32,
                    name,
                    cpu_usage,
                    memory_usage,
                    last_seen: SystemTime::now(),
                });
            }
        }
        
        std::thread::sleep(std::time::Duration::from_secs(2));
    }
}