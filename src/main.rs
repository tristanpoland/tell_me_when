use tell_me_when::{
    EventSystem, 
    FsEventData, FsEventType,
    ProcessEventData, ProcessEventType,
    NetworkEventData, NetworkEventType,
    SystemEventData, SystemEventType,
    PowerEventData, PowerEventType
};
use std::path::Path;
use tokio::signal;
use colored::*;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging to see debug output
    env_logger::init();
    
    println!("🚀 Starting tell_me_when comprehensive event monitoring");
    println!("This will show real-time OS callbacks - no polling!");
    println!("Monitoring: Filesystem (C:), Process, Network, System, Power events");
    println!("Press Ctrl+C to stop...\n");
    
    // Create and start the event system
    let mut event_system = EventSystem::new();
    event_system.start().await?;
    
    // Monitor the entire C: drive for file creation events
    let c_drive = Path::new("C:\\");
    
    println!("📁 Setting up filesystem monitoring for: {}", c_drive.display());
    
    // Listen for file creation events
    let _creation_id = event_system.on_fs_created(c_drive, |event: FsEventData| {
        println!("{}", format!("✨ [CREATE] {}", event.path.display()).bright_blue());
    }).await?;
    
    // Listen for file modification events  
    let _modification_id = event_system.on_fs_modified(c_drive, |event: FsEventData| {
        println!("{}", format!("📝 [MODIFY] {}", event.path.display()).bright_yellow());
    }).await?;
    
    // Listen for file deletion events
    let _deletion_id = event_system.on_fs_deleted(c_drive, |event: FsEventData| {
        println!("{}", format!("🗑️  [DELETE] {}", event.path.display()).bright_red());
    }).await?;
    
    // Listen for ALL filesystem events with more detailed info
    let _all_events_id = event_system.on_fs_event(c_drive, |event: FsEventData| {
        let (event_icon, color_fn): (&str, fn(&str) -> ColoredString) = match event.event_type {
            FsEventType::Created => ("🆕", |s| s.bright_blue()),
            FsEventType::Modified => ("📋", |s| s.bright_yellow()),
            FsEventType::Deleted => ("❌", |s| s.bright_red()),
            FsEventType::Renamed { .. } => ("🔄", |s| s.bright_purple()),
            FsEventType::Moved { .. } => ("📦", |s| s.bright_cyan()),
            FsEventType::AttributeChanged => ("⚙️", |s| s.bright_green()),
            FsEventType::PermissionChanged => ("🔒", |s| s.bright_magenta()),
        };
        
        let timestamp = event.timestamp
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
            
        let output = format!("{} [{:?}] {} ({}ms)", 
            event_icon, 
            event.event_type, 
            event.path.display(),
            timestamp % 100000 // Show last 5 digits for brevity
        );
        
        println!("{}", color_fn(&output));
    }).await?;
    
    // Add process monitoring
    let _process_id = event_system.on_process_event(|event: ProcessEventData| {
        let (event_icon, color_fn): (&str, fn(&str) -> ColoredString) = match event.event_type {
            ProcessEventType::Started => ("🟢", |s| s.bright_green()),
            ProcessEventType::Terminated => ("🔴", |s| s.bright_red()),
            ProcessEventType::CpuUsageHigh => ("🔥", |s| s.red()),
            ProcessEventType::MemoryUsageHigh => ("💾", |s| s.yellow()),
            ProcessEventType::StatusChanged => ("🔄", |s| s.white()),
        };
        
        let output = format!("{} [PROCESS] {} (PID: {}) - {:?}", 
            event_icon, 
            event.name, 
            event.pid,
            event.event_type
        );
        
        println!("{}", color_fn(&output));
    }).await?;
    
    // Add network monitoring
    // let _network_id = event_system.on_network_event(|event: NetworkEventData| {
    //     let (event_icon, color_fn): (&str, fn(&str) -> ColoredString) = match event.event_type {
    //         NetworkEventType::InterfaceUp => ("📶", |s| s.bright_green()),
    //         NetworkEventType::InterfaceDown => ("📵", |s| s.bright_red()),
    //         NetworkEventType::ConnectionEstablished => ("🔗", |s| s.bright_cyan()),
    //         NetworkEventType::ConnectionLost => ("🔗", |s| s.red()),
    //         NetworkEventType::TrafficThresholdReached => ("📊", |s| s.bright_yellow()),
    //     };
    //     
    //     let interface = event.interface_name.as_deref().unwrap_or("unknown");
    //     let output = format!("{} [NETWORK] {} - {:?}", 
    //         event_icon, 
    //         interface,
    //         event.event_type
    //     );
    //     
    //     println!("{}", color_fn(&output));
    // }).await?;
    
    // Add system monitoring
    // let _system_id = event_system.on_system_event(|event: SystemEventData| {
    //     let (event_icon, color_fn): (&str, fn(&str) -> ColoredString) = match event.event_type {
    //         SystemEventType::CpuUsageHigh => ("🔥", |s| s.red()),
    //         SystemEventType::MemoryUsageHigh => ("💾", |s| s.yellow()),
    //         SystemEventType::DiskSpaceLow => ("💽", |s| s.bright_red()),
    //         SystemEventType::TemperatureHigh => ("🌡️", |s| s.red()),
    //         SystemEventType::LoadAverageHigh => ("⚡", |s| s.bright_yellow()),
    //     };
    //     
    //     let output = format!("{} [SYSTEM] {:?}", 
    //         event_icon, 
    //         event.event_type
    //     );
    //     
    //     println!("{}", color_fn(&output));
    // }).await?;
    
    // Add power monitoring
    let _power_id = event_system.on_power_event(|event: PowerEventData| {
        let (event_icon, color_fn): (&str, fn(&str) -> ColoredString) = match event.event_type {
            PowerEventType::BatteryLow => ("🪫", |s| s.bright_red()),
            PowerEventType::BatteryCharging => ("🔋", |s| s.bright_green()),
            PowerEventType::BatteryDischarging => ("🔋", |s| s.yellow()),
            PowerEventType::PowerSourceChanged => ("🔌", |s| s.bright_cyan()),
            PowerEventType::SleepMode => ("😴", |s| s.blue()),
            PowerEventType::WakeFromSleep => ("👁️", |s| s.bright_blue()),
            PowerEventType::Shutdown => ("🛑", |s| s.bright_red()),
            PowerEventType::Restart => ("🔄", |s| s.bright_magenta()),
        };
        
        let output = format!("{} [POWER] {:?}", 
            event_icon, 
            event.event_type
        );
        
        println!("{}", color_fn(&output));
    }).await?;
    
    println!("🎯 Comprehensive event monitoring active! Waiting for OS callbacks...");
    println!("🗂️  Filesystem: Try creating, modifying, or deleting files on C: drive");
    println!("⚙️  Process: Start/stop applications to see process events");
    println!("🌐 Network: Connect/disconnect network interfaces");
    println!("🖥️  System: High CPU/memory usage will trigger system events");
    println!("🔋 Power: Battery and power source changes will be monitored");
    println!("Events will appear instantly via native OS callbacks\n");
    
    // Wait for Ctrl+C to stop
    signal::ctrl_c().await?;
    
    println!("\n🛑 Stopping filesystem monitoring...");
    event_system.stop().await?;
    println!("✅ Event system stopped successfully");
    
    Ok(())
}