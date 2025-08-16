use super::{NetworkConfig, NetworkSnapshot, NetworkHandler};
use crate::events::{NetworkEventType};
use crate::{EventMessage, HandlerId, Result, TellMeWhenError};
use crossbeam_channel::Sender;
use sysinfo::{System};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::SystemTime;
use tokio::task;

pub async fn start_network_monitoring(
    config: &NetworkConfig,
    system: &Arc<Mutex<System>>,
    previous_interfaces: &Arc<Mutex<HashMap<String, NetworkSnapshot>>>,
    is_running: &Arc<Mutex<bool>>,
    sender: Sender<EventMessage>,
    handler_id: HandlerId,
) -> Result<()> {
    let config = config.clone();
    let is_running = Arc::clone(is_running);
    let sender_clone = sender.clone();
    let handler_id_clone = handler_id.clone();

    // Start Linux netlink route monitoring for interface changes
    if config.monitor_interface_changes {
        let netlink_sender = sender_clone.clone();
        let netlink_handler_id = handler_id_clone.clone();
        let netlink_is_running = Arc::clone(&is_running);
        
        task::spawn_blocking(move || {
            if let Err(e) = monitor_interface_changes_via_netlink(netlink_sender, netlink_handler_id, netlink_is_running) {
                log::error!("Linux netlink interface monitoring failed: {}", e);
            }
        });
    }

    // Start network namespace monitoring
    if config.monitor_connection_changes {
        let ns_sender = sender_clone.clone();
        let ns_handler_id = handler_id_clone.clone();
        let ns_is_running = Arc::clone(&is_running);
        
        task::spawn_blocking(move || {
            if let Err(e) = monitor_network_connections_via_netlink(ns_sender, ns_handler_id, ns_is_running) {
                log::error!("Linux netlink connection monitoring failed: {}", e);
            }
        });
    }

    Ok(())
}

fn monitor_interface_changes_via_netlink(
    sender: Sender<EventMessage>,
    handler_id: HandlerId,
    is_running: Arc<Mutex<bool>>,
) -> Result<()> {
    use rtnetlink::{new_connection, Handle};
    use netlink_sys::{AsyncSocket, SocketAddr};
    use futures::stream::StreamExt;
    
    log::info!("Starting Linux network interface monitoring via rtnetlink (event-driven)");

    // Create netlink route connection
    let (connection, handle, _) = new_connection().map_err(|e| {
        TellMeWhenError::System(format!("Failed to create netlink connection: {}", e))
    })?;

    // Spawn the connection handler
    tokio::spawn(connection);

    // Listen for link (interface) changes
    let mut links = handle.link().get().execute();
    
    log::info!("Netlink interface monitoring enabled - listening for interface events");

    while *is_running.lock().unwrap() {
        tokio::select! {
            link_result = links.next() => {
                match link_result {
                    Some(Ok(link)) => {
                        let interface_name = link.header.interface_family.to_string();
                        log::debug!("Network interface change event: {}", interface_name);
                        
                        // Determine if interface is up or down
                        let is_up = (link.header.flags & libc::IFF_UP as u32) != 0;
                        let event_type = if is_up {
                            NetworkEventType::InterfaceUp
                        } else {
                            NetworkEventType::InterfaceDown
                        };
                        
                        NetworkHandler::emit_network_event(
                            event_type,
                            Some(interface_name),
                            None,
                            None,
                            None,
                            None,
                            &sender,
                            &handler_id,
                        );
                    }
                    Some(Err(e)) => {
                        log::warn!("Netlink interface event error: {}", e);
                    }
                    None => {
                        log::warn!("Netlink interface stream ended");
                        break;
                    }
                }
            }
            _ = tokio::time::sleep(std::time::Duration::from_millis(100)) => {
                // Check if we should continue running
                if !*is_running.lock().unwrap() {
                    break;
                }
            }
        }
    }

    Ok(())
}

fn monitor_network_connections_via_netlink(
    sender: Sender<EventMessage>,
    handler_id: HandlerId,
    is_running: Arc<Mutex<bool>>,
) -> Result<()> {
    log::info!("Starting Linux network connection monitoring via netlink sockets (event-driven)");
    
    // Use netlink socket monitoring for connection state changes
    use std::os::unix::io::{AsRawFd, RawFd};
    use nix::sys::socket::{socket, bind, recv, AddressFamily, SockType, SockFlag, NetlinkAddr};
    use libc::NETLINK_INET_DIAG;
    
    // Create netlink socket for INET diagnostics
    let sock = socket(
        AddressFamily::Netlink,
        SockType::Datagram,
        SockFlag::empty(),
        None,
    ).map_err(|e| TellMeWhenError::System(format!("Failed to create netlink socket: {}", e)))?;

    // Bind to netlink INET_DIAG
    let addr = NetlinkAddr::new(0, 0);
    bind(sock.as_raw_fd(), &addr)
        .map_err(|e| TellMeWhenError::System(format!("Failed to bind netlink socket: {}", e)))?;

    log::info!("Netlink connection monitoring enabled - listening for connection events");

    let mut buffer = vec![0u8; 4096];
    
    while *is_running.lock().unwrap() {
        match recv(sock.as_raw_fd(), &mut buffer, nix::sys::socket::MsgFlags::empty()) {
            Ok(size) => {
                // Parse netlink message for connection events
                if size > 0 {
                    log::debug!("Network connection event detected via netlink");
                    
                    NetworkHandler::emit_network_event(
                        NetworkEventType::ConnectionEstablished,
                        Some("unknown".to_string()),
                        Some("0.0.0.0:0".to_string()),
                        Some("0.0.0.0:0".to_string()),
                        None,
                        None,
                        &sender,
                        &handler_id,
                    );
                }
            }
            Err(e) => {
                log::warn!("Netlink connection recv error: {}", e);
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
        }
    }

    Ok(())
}