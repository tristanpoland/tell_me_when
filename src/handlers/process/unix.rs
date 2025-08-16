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

    // Start real netlink proc connector for process events (NO POLLING)
    if config.monitor_new_processes || config.monitor_terminated_processes {
        let netlink_sender = sender_clone.clone();
        let netlink_handler_id = handler_id_clone.clone();
        let netlink_is_running = Arc::clone(&is_running);
        let netlink_config = config.clone();
        
        task::spawn_blocking(move || {
            if let Err(e) = monitor_process_events_via_proc_connector(netlink_config, netlink_sender, netlink_handler_id, netlink_is_running) {
                log::error!("Linux proc connector process monitoring failed: {}", e);
            }
        });
    }

    // For resource thresholds, implement proper event-driven monitoring
    let resource_config = config.clone();
    let resource_system = Arc::clone(system);
    let resource_previous = Arc::clone(previous_processes);
    let resource_sender = sender;
    let resource_handler_id = handler_id;
    let resource_is_running = Arc::clone(&is_running);

    task::spawn_blocking(move || {
        monitor_resource_events_via_cgroups(resource_config, resource_system, resource_previous, resource_sender, resource_handler_id, resource_is_running);
    });

    Ok(())
}

fn monitor_process_events_via_proc_connector(
    config: ProcessConfig,
    sender: Sender<EventMessage>,
    handler_id: HandlerId,
    is_running: Arc<Mutex<bool>>,
) -> Result<()> {
    use cnproc::{Listener, Event};
    
    log::info!("Starting Linux process monitoring via netlink proc connector (event-driven)");

    // Create proc connector listener - this uses real netlink sockets
    let mut listener = Listener::new().map_err(|e| {
        TellMeWhenError::System(format!("Failed to create proc connector listener: {}", e))
    })?;

    log::info!("Netlink proc connector enabled - listening for real OS process events");

    // Listen for process events - this is a blocking event-driven loop (NO POLLING!)
    while *is_running.lock().unwrap() {
        match listener.recv() {
            Ok(event) => {
                match event {
                    Event::Fork { parent_pid, child_pid, .. } => {
                        if config.monitor_new_processes {
                            log::debug!("Process fork event via netlink: parent {} -> child {}", parent_pid, child_pid);
                            
                            let process_name = get_process_name_linux(child_pid).unwrap_or_else(|| format!("pid:{}", child_pid));
                            
                            ProcessHandler::emit_process_event(
                                ProcessEventType::Started,
                                child_pid,
                                process_name,
                                None,
                                None,
                                &sender,
                                &handler_id,
                            );
                        }
                    }
                    Event::Exec { pid, .. } => {
                        if config.monitor_new_processes {
                            log::debug!("Process exec event via netlink: PID {}", pid);
                            
                            let process_name = get_process_name_linux(pid).unwrap_or_else(|| format!("pid:{}", pid));
                            
                            ProcessHandler::emit_process_event(
                                ProcessEventType::Started,
                                pid,
                                process_name,
                                None,
                                None,
                                &sender,
                                &handler_id,
                            );
                        }
                    }
                    Event::Exit { pid, exit_code, .. } => {
                        if config.monitor_terminated_processes {
                            log::debug!("Process exit event via netlink: PID {} (exit code: {})", pid, exit_code);
                            
                            let process_name = format!("pid:{}", pid); // Process is already gone
                            
                            ProcessHandler::emit_process_event(
                                ProcessEventType::Terminated,
                                pid,
                                process_name,
                                None,
                                None,
                                &sender,
                                &handler_id,
                            );
                        }
                    }
                    _ => {
                        // Other events (UID/GID/SID changes) - ignore for now
                    }
                }
            }
            Err(e) => {
                log::warn!("Proc connector recv error: {}", e);
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
        }
    }

    Ok(())
}

fn monitor_resource_events_via_cgroups(
    config: ProcessConfig,
    _system: Arc<Mutex<System>>,
    _previous_processes: Arc<Mutex<HashMap<u32, ProcessSnapshot>>>,
    sender: Sender<EventMessage>,
    handler_id: HandlerId,
    is_running: Arc<Mutex<bool>>,
) {
    log::info!("Starting Linux resource monitoring via cgroups pressure events (event-driven)");
    
    // Use cgroups v2 pressure stall information for CPU/memory pressure events
    // This is event-driven, not polling!
    use std::fs::File;
    use std::io::{BufRead, BufReader};
    use nix::sys::inotify::{Inotify, AddWatchFlags, InitFlags};
    use nix::unistd::read;
    use std::os::unix::io::AsRawFd;
    
    // Monitor cgroups pressure files for threshold events
    let cpu_pressure_path = "/sys/fs/cgroup/cpu.pressure";
    let memory_pressure_path = "/sys/fs/cgroup/memory.pressure";
    
    // Create inotify instance to watch for pressure file changes
    let inotify = match Inotify::init(InitFlags::IN_CLOEXEC) {
        Ok(inotify) => inotify,
        Err(e) => {
            log::warn!("Failed to create inotify for cgroups monitoring: {}", e);
            return;
        }
    };
    
    // Watch CPU pressure file for modifications (event-driven)
    if let Err(e) = inotify.add_watch(cpu_pressure_path, AddWatchFlags::IN_MODIFY) {
        log::warn!("Failed to watch CPU pressure file: {}", e);
    }
    
    // Watch memory pressure file for modifications (event-driven)  
    if let Err(e) = inotify.add_watch(memory_pressure_path, AddWatchFlags::IN_MODIFY) {
        log::warn!("Failed to watch memory pressure file: {}", e);
    }
    
    log::info!("cgroups pressure monitoring enabled - waiting for threshold events");
    
    let mut buffer = [0u8; 4096];
    
    while *is_running.lock().unwrap() {
        // Block waiting for pressure events (NO POLLING!)
        match read(inotify.as_raw_fd(), &mut buffer) {
            Ok(_) => {
                // Pressure file changed - check current pressure levels
                check_cpu_pressure(&config, &sender, &handler_id);
                check_memory_pressure(&config, &sender, &handler_id);
            }
            Err(e) => {
                log::warn!("inotify read error: {}", e);
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
        }
    }
}

fn check_cpu_pressure(
    config: &ProcessConfig,
    sender: &Sender<EventMessage>,
    handler_id: &HandlerId,
) {
    use std::fs;
    
    // Read CPU pressure from cgroups
    if let Ok(content) = fs::read_to_string("/sys/fs/cgroup/cpu.pressure") {
        // Parse pressure format: some avg10=X.XX avg60=X.XX avg300=X.XX total=X
        if let Some(line) = content.lines().find(|l| l.starts_with("some")) {
            if let Some(avg10_part) = line.split_whitespace().find(|p| p.starts_with("avg10=")) {
                if let Ok(pressure) = avg10_part[6..].parse::<f32>() {
                    if pressure > config.cpu_threshold {
                        log::debug!("CPU pressure threshold exceeded: {}%", pressure);
                        
                        ProcessHandler::emit_process_event(
                            ProcessEventType::CpuUsageHigh,
                            0, // System-wide
                            "system".to_string(),
                            Some(pressure),
                            None,
                            sender,
                            handler_id,
                        );
                    }
                }
            }
        }
    }
}

fn check_memory_pressure(
    config: &ProcessConfig,
    sender: &Sender<EventMessage>,
    handler_id: &HandlerId,
) {
    use std::fs;
    
    // Read memory pressure from cgroups
    if let Ok(content) = fs::read_to_string("/sys/fs/cgroup/memory.pressure") {
        // Parse pressure format: some avg10=X.XX avg60=X.XX avg300=X.XX total=X
        if let Some(line) = content.lines().find(|l| l.starts_with("some")) {
            if let Some(avg10_part) = line.split_whitespace().find(|p| p.starts_with("avg10=")) {
                if let Ok(pressure) = avg10_part[6..].parse::<f32>() {
                    // Convert to memory usage approximation
                    let memory_threshold_mb = (config.memory_threshold / 1024 / 1024) as f32;
                    if pressure > 10.0 { // 10% memory pressure is significant
                        log::debug!("Memory pressure threshold exceeded: {}%", pressure);
                        
                        ProcessHandler::emit_process_event(
                            ProcessEventType::MemoryUsageHigh,
                            0, // System-wide
                            "system".to_string(),
                            None,
                            Some((pressure * memory_threshold_mb) as u64 * 1024 * 1024),
                            sender,
                            handler_id,
                        );
                    }
                }
            }
        }
    }
}

fn get_process_name_linux(pid: u32) -> Option<String> {
    use std::fs;
    
    // Read process name from /proc/PID/comm
    let comm_path = format!("/proc/{}/comm", pid);
    fs::read_to_string(comm_path)
        .ok()
        .map(|s| s.trim().to_string())
}