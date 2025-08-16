use tell_me_when::{EventSystem, FsEventData, ProcessEventData, SystemEventData, NetworkEventData, PowerEventData};
use std::path::Path;
use tokio::time::{sleep, Duration};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    env_logger::init();

    // Create a new event system
    let mut event_system = EventSystem::new();
    
    // Start the event system
    event_system.start().await?;

    println!("🚀 Tell Me When - Event System Started!");
    println!("Setting up event listeners...\n");

    // File system events
    let _fs_id = event_system.on_fs_created("./", |event: FsEventData| {
        println!("📁 File created: {:?} at {:?}", event.path, event.timestamp);
    }).await?;

    let _fs_modified_id = event_system.on_fs_modified("./", |event: FsEventData| {
        println!("📝 File modified: {:?}", event.path);
    }).await?;

    let _fs_deleted_id = event_system.on_fs_deleted("./", |event: FsEventData| {
        println!("🗑️  File deleted: {:?}", event.path);
    }).await?;

    // Process events
    let _process_started_id = event_system.on_process_started(|event: ProcessEventData| {
        println!("🔄 Process started: {} (PID: {})", event.name, event.pid);
    }).await?;

    let _process_terminated_id = event_system.on_process_terminated(|event: ProcessEventData| {
        println!("❌ Process terminated: {} (PID: {})", event.name, event.pid);
    }).await?;

    // System resource events with thresholds
    // let _cpu_high_id = event_system.on_cpu_usage_high(75.0, |event: SystemEventData| {
    //     if let Some(cpu_usage) = event.cpu_usage {
    //         println!("🔥 High CPU usage detected: {:.1}%", cpu_usage);
    //     }
    // }).await?;

    //let _memory_high_id = event_system.on_memory_usage_high(80.0, |event: SystemEventData| {
    //    if let Some(memory_usage) = event.memory_usage {
    //        println!("💾 High memory usage detected: {:.1}%", memory_usage);
    //    }
    //}).await?;

    // Network events
    // let _network_id = event_system.on_network_event(|event: NetworkEventData| {
    //     match event.event_type {
    //         tell_me_when::NetworkEventType::InterfaceUp => {
    //             println!("🌐 Network interface up: {:?}", event.interface_name);
    //         }
    //         tell_me_when::NetworkEventType::InterfaceDown => {
    //             println!("📡 Network interface down: {:?}", event.interface_name);
    //         }
    //         tell_me_when::NetworkEventType::TrafficThresholdReached => {
    //             println!("🚀 High network traffic on: {:?}", event.interface_name);
    //         }
    //         _ => {}
    //     }
    // }).await?;

    // Power events
    let _battery_low_id = event_system.on_battery_low(25.0, |event: PowerEventData| {
        if let Some(battery_level) = event.battery_level {
            println!("🔋 Low battery warning: {:.1}%", battery_level);
        }
    }).await?;

    let _power_id = event_system.on_power_event(|event: PowerEventData| {
        match event.event_type {
            tell_me_when::PowerEventType::BatteryCharging => {
                println!("🔌 Battery started charging");
            }
            tell_me_when::PowerEventType::BatteryDischarging => {
                println!("🔋 Battery stopped charging");
            }
            tell_me_when::PowerEventType::PowerSourceChanged => {
                println!("⚡ Power source changed to: {:?}", event.power_source);
            }
            _ => {}
        }
    }).await?;

    println!("✅ All event listeners are active!");
    println!("🎯 Try creating, modifying, or deleting files in the current directory");
    println!("📊 Monitor system resources and watch for threshold breaches");
    println!("🔌 Plug/unplug power adapter to see power events");
    println!("\nPress Ctrl+C to stop...\n");

    // Demonstration: Create and modify a test file
    tokio::spawn(async {
        sleep(Duration::from_secs(2)).await;
        
        // Create a test file
        if let Err(e) = std::fs::write("test_file.txt", "Hello, Tell Me When!") {
            eprintln!("Failed to create test file: {}", e);
        }
        
        sleep(Duration::from_secs(1)).await;
        
        // Modify the test file
        if let Err(e) = std::fs::write("test_file.txt", "Hello, Tell Me When! - Modified") {
            eprintln!("Failed to modify test file: {}", e);
        }
        
        sleep(Duration::from_secs(1)).await;
        
        // Delete the test file
        if let Err(e) = std::fs::remove_file("test_file.txt") {
            eprintln!("Failed to delete test file: {}", e);
        }
    });

    // Keep the program running to demonstrate event handling
    // In a real application, you'd have your main application logic here
    loop {
        sleep(Duration::from_secs(1)).await;
    }
}

// Example of advanced usage with custom filtering
#[allow(dead_code)]
async fn advanced_example() -> Result<(), Box<dyn std::error::Error>> {
    let mut event_system = EventSystem::new();
    event_system.start().await?;

    // Watch specific file types only
    let _rust_files_id = event_system.on_fs_event("./src", |event: FsEventData| {
        let path_str = event.path.to_string_lossy();
        if path_str.ends_with(".rs") {
            println!("🦀 Rust file changed: {:?} -> {:?}", event.path, event.event_type);
        }
    }).await?;

    // Monitor specific processes
    let _cargo_process_id = event_system.on_process_event(|event: ProcessEventData| {
        if event.name.contains("cargo") || event.name.contains("rustc") {
            println!("🔧 Rust toolchain process: {} ({})", event.name, event.event_type);
        }
    }).await?;

    // System health monitoring with custom thresholds
    // let _system_health_id = event_system.on_system_event(|event: SystemEventData| {
    //     match event.event_type {
    //         tell_me_when::SystemEventType::CpuUsageHigh => {
    //             println!("⚠️  System performance warning: High CPU usage");
    //         }
    //         tell_me_when::SystemEventType::MemoryUsageHigh => {
    //             println!("⚠️  System performance warning: High memory usage");
    //         }
    //         tell_me_when::SystemEventType::DiskSpaceLow => {
    //             println!("⚠️  Storage warning: Low disk space");
    //         }
    //         tell_me_when::SystemEventType::TemperatureHigh => {
    //             println!("🌡️  Hardware warning: High temperature");
    //         }
    //         _ => {}
    //     }
    // }).await?;

    Ok(())
}