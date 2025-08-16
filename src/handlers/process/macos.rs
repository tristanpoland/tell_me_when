use super::{ProcessConfig, ProcessSnapshot, ProcessHandler};
use crate::events::{ProcessEventType};
use crate::{EventMessage, HandlerId, Result, TellMeWhenError};
use crossbeam_channel::Sender;
use sysinfo::{System, Pid, Process};
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
    let system = Arc::clone(system);
    let previous_processes = Arc::clone(previous_processes);
    let is_running = Arc::clone(is_running);

    // Use kqueue for macOS process monitoring
    task::spawn_blocking(move || {
        monitor_via_kqueue(&config, &system, &previous_processes, &is_running, sender, handler_id);
    }).await.map_err(|e| TellMeWhenError::RuntimeError(format!("Process monitoring task failed: {}", e)))?;

    Ok(())
}

fn monitor_via_kqueue(
    config: &ProcessConfig,
    system: &Arc<Mutex<System>>,
    previous_processes: &Arc<Mutex<HashMap<u32, ProcessSnapshot>>>,
    is_running: &Arc<Mutex<bool>>,
    sender: Sender<EventMessage>,
    handler_id: HandlerId,
) {
    log::info!("Starting macOS process monitoring via kqueue and sysinfo");
    
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
                
                // Check if this is a new process
                if !prev_processes.contains_key(&pid_u32) && config.monitor_new_processes {
                    log::debug!("New process detected: {} (PID: {})", name, pid_u32);
                    ProcessHandler::emit_process_event(
                        ProcessEventType::Started,
                        pid_u32,
                        name.clone(),
                        Some(cpu_usage),
                        Some(memory_usage),
                        &sender,
                        &handler_id,
                    );
                }
                
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
            
            // Check for terminated processes
            if config.monitor_terminated_processes {
                let mut terminated_pids = Vec::new();
                for (old_pid, snapshot) in prev_processes.iter() {
                    if !current_pids.contains(old_pid) {
                        terminated_pids.push(*old_pid);
                        log::debug!("Process terminated: {} (PID: {})", snapshot.name, old_pid);
                        ProcessHandler::emit_process_event(
                            ProcessEventType::Terminated,
                            *old_pid,
                            snapshot.name.clone(),
                            None,
                            None,
                            &sender,
                            &handler_id,
                        );
                    }
                }
                
                // Remove terminated processes from tracking
                for pid in terminated_pids {
                    prev_processes.remove(&pid);
                }
            }
        }
        
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
}