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

    // Start kqueue process monitoring for real-time process events
    let kqueue_sender = sender_clone.clone();
    let kqueue_handler_id = handler_id_clone.clone();
    let kqueue_is_running = Arc::clone(&is_running);
    let kqueue_config = config.clone();
    
    task::spawn_blocking(move || {
        if let Err(e) = monitor_process_events_via_kqueue(kqueue_config, kqueue_sender, kqueue_handler_id, kqueue_is_running) {
            log::error!("macOS kqueue process monitoring failed: {}", e);
        }
    });

    Ok(())
}

fn monitor_process_events_via_kqueue(
    config: ProcessConfig,
    sender: Sender<EventMessage>,
    handler_id: HandlerId,
    is_running: Arc<Mutex<bool>>,
) -> Result<()> {
    use std::os::unix::io::AsRawFd;
    use nix::sys::event::{kqueue, kevent, EventFilter, FilterFlag, EventFlag};
    use nix::sys::signal::Signal;
    
    log::info!("Starting macOS process monitoring via kqueue EVFILT_PROC and EVFILT_SIGNAL");

    // Create kqueue instance
    let kq = kqueue().map_err(|e| TellMeWhenError::System(format!("Failed to create kqueue: {}", e)))?;

    let mut changes = Vec::new();
    
    // Monitor SIGCHLD for process termination (event-driven)
    if config.monitor_terminated_processes {
        changes.push(kevent(
            Signal::SIGCHLD as usize,
            EventFilter::EVFILT_SIGNAL,
            EventFlag::EV_ADD | EventFlag::EV_ENABLE,
            FilterFlag::empty(),
            0,
            0,
        ));
    }

    // Monitor process creation via EVFILT_PROC (event-driven)
    if config.monitor_new_processes {
        // This monitors system-wide process creation events
        changes.push(kevent(
            1, // kernel task pid
            EventFilter::EVFILT_PROC,
            EventFlag::EV_ADD | EventFlag::EV_ENABLE,
            FilterFlag::NOTE_FORK | FilterFlag::NOTE_EXEC,
            0,
            0,
        ));
    }

    // Register events
    if !changes.is_empty() {
        kevent(kq.as_raw_fd(), &changes, &mut [], None)
            .map_err(|e| TellMeWhenError::System(format!("Failed to register kqueue events: {}", e)))?;
    }

    log::info!("kqueue process monitoring enabled - pure event callbacks, NO POLLING!");

    // Listen for events - this blocks until events occur (NO POLLING!)
    let mut events = vec![kevent(0, EventFilter::EVFILT_SIGNAL, EventFlag::empty(), FilterFlag::empty(), 0, 0); 10];
    
    while *is_running.lock().unwrap() {
        match kevent(kq.as_raw_fd(), &[], &mut events, Some(std::time::Duration::from_millis(1000))) {
            Ok(num_events) => {
                for i in 0..num_events {
                    let event = &events[i];
                    
                    if event.filter() == EventFilter::EVFILT_SIGNAL && event.ident() == Signal::SIGCHLD as usize {
                        // Process termination event
                        handle_sigchld(&config, &sender, &handler_id);
                    } else if event.filter() == EventFilter::EVFILT_PROC {
                        // Process creation event
                        let pid = event.ident() as u32;
                        
                        if event.fflags().contains(FilterFlag::NOTE_FORK) {
                            log::debug!("Process fork event via kqueue: PID {}", pid);
                            emit_process_start_event(pid, &sender, &handler_id);
                        }
                        
                        if event.fflags().contains(FilterFlag::NOTE_EXEC) {
                            log::debug!("Process exec event via kqueue: PID {}", pid);
                            emit_process_start_event(pid, &sender, &handler_id);
                        }
                    }
                }
            }
            Err(e) => {
                log::warn!("kqueue kevent error: {}", e);
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
        }
    }

    Ok(())
}

fn emit_process_start_event(
    pid: u32,
    sender: &Sender<EventMessage>,
    handler_id: &HandlerId,
) {
    let process_name = get_process_name_macos(pid).unwrap_or_else(|| format!("pid:{}", pid));
    
    ProcessHandler::emit_process_event(
        ProcessEventType::Started,
        pid,
        process_name,
        None,
        None,
        sender,
        handler_id,
    );
}

fn handle_sigchld(
    config: &ProcessConfig,
    sender: &Sender<EventMessage>,
    handler_id: &HandlerId,
) {
    use nix::sys::wait::{waitpid, WaitPidFlag, WaitStatus};
    use nix::unistd::Pid;
    
    // Reap zombie processes and emit termination events (event-driven, not polling)
    loop {
        match waitpid(Pid::from_raw(-1), Some(WaitPidFlag::WNOHANG)) {
            Ok(WaitStatus::Exited(pid, _)) => {
                let pid_u32 = pid.as_raw() as u32;
                log::debug!("Process terminated via SIGCHLD: PID {}", pid_u32);
                
                ProcessHandler::emit_process_event(
                    ProcessEventType::Terminated,
                    pid_u32,
                    format!("pid:{}", pid_u32),
                    None,
                    None,
                    sender,
                    handler_id,
                );
            }
            Ok(WaitStatus::Signaled(pid, _, _)) => {
                let pid_u32 = pid.as_raw() as u32;
                log::debug!("Process killed via SIGCHLD: PID {}", pid_u32);
                
                ProcessHandler::emit_process_event(
                    ProcessEventType::Terminated,
                    pid_u32,
                    format!("pid:{}", pid_u32),
                    None,
                    None,
                    sender,
                    handler_id,
                );
            }
            Ok(WaitStatus::StillAlive) => {
                // No more zombie processes
                break;
            }
            Ok(_) => {
                // Other wait statuses, continue
                continue;
            }
            Err(_) => {
                // No more child processes or error
                break;
            }
        }
    }
}

fn get_process_name_macos(pid: u32) -> Option<String> {
    use std::process::Command;
    
    // Use ps command to get process name
    Command::new("ps")
        .args(&["-p", &pid.to_string(), "-o", "comm="])
        .output()
        .ok()
        .and_then(|output| {
            if output.status.success() {
                String::from_utf8(output.stdout).ok().map(|s| s.trim().to_string())
            } else {
                None
            }
        })
}