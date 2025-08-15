# Tell Me When

A Rust library for cross-platform event monitoring and callback-based notifications. Monitor file system changes, process events, system resource usage, network changes, and power events with simple, intuitive APIs.

[![Crates.io](https://img.shields.io/crates/v/tell_me_when.svg)](https://crates.io/crates/tell_me_when)
[![Documentation](https://docs.rs/tell_me_when/badge.svg)](https://docs.rs/tell_me_when)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

## Features

- 🔄 **Cross-platform**: Windows, macOS, and Linux support with native APIs
- 📁 **File System Events**: Monitor file/directory creation, modification, deletion, and attribute changes
- 🔧 **Process Monitoring**: Track process lifecycle, CPU/memory usage, and status changes
- 💾 **System Resources**: Monitor CPU usage, memory consumption, disk space, and temperature
- 🌐 **Network Events**: Track interface changes and traffic thresholds
- 🔋 **Power Management**: Monitor battery level, charging state, and power source changes
- ⚡ **High Performance**: Efficient native implementations with minimal overhead
- 🛡️ **Type Safe**: Full Rust type safety with comprehensive error handling
- 🔧 **Modular Design**: Easy to extend with custom event handlers
- 📊 **Threshold-based**: Configurable thresholds for resource monitoring

## Quick Start

Add to your `Cargo.toml`:

```toml
[dependencies]
tell_me_when = "0.1.0"
tokio = { version = "1.0", features = ["full"] }
```

## Basic Usage

```rust
use tell_me_when::EventSystem;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut event_system = EventSystem::new();
    event_system.start().await?;

    // File system events
    event_system.on_fs_created("./", |event| {
        println!("File created: {:?}", event.path);
    }).await?;

    event_system.on_fs_modified("./src", |event| {
        println!("Source file modified: {:?}", event.path);
    }).await?;

    // Process monitoring
    event_system.on_process_started(|event| {
        println!("New process: {} (PID: {})", event.name, event.pid);
    }).await?;

    // System resource monitoring with thresholds
    event_system.on_cpu_usage_high(80.0, |event| {
        println!("High CPU usage: {:.1}%", event.cpu_usage.unwrap_or(0.0));
    }).await?;

    event_system.on_memory_usage_high(85.0, |event| {
        println!("High memory usage: {:.1}%", event.memory_usage.unwrap_or(0.0));
    }).await?;

    // Power events
    event_system.on_battery_low(20.0, |event| {
        println!("Low battery: {:.1}%", event.battery_level.unwrap_or(0.0));
    }).await?;

    // Network events
    event_system.on_network_event(|event| {
        match event.event_type {
            tell_me_when::NetworkEventType::InterfaceUp => {
                println!("Network interface up: {:?}", event.interface_name);
            }
            tell_me_when::NetworkEventType::InterfaceDown => {
                println!("Network interface down: {:?}", event.interface_name);
            }
            _ => {}
        }
    }).await?;

    // Keep running
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }
}
```

## Event Types

### File System Events

Monitor file and directory changes with precise event types:

```rust
// Specific event types
event_system.on_fs_created("./docs", |event| {
    println!("New file: {:?}", event.path);
}).await?;

event_system.on_fs_modified("./src", |event| {
    println!("File changed: {:?}", event.path);
}).await?;

event_system.on_fs_deleted("./tmp", |event| {
    println!("File deleted: {:?}", event.path);
}).await?;

// General file system events with filtering
event_system.on_fs_event("./", |event| {
    match event.event_type {
        FsEventType::Created => println!("Created: {:?}", event.path),
        FsEventType::Modified => println!("Modified: {:?}", event.path),
        FsEventType::Deleted => println!("Deleted: {:?}", event.path),
        FsEventType::Renamed { old_path, new_path } => {
            println!("Renamed: {:?} -> {:?}", old_path, new_path);
        }
        _ => {}
    }
}).await?;
```

### Process Events

Track system processes and their resource usage:

```rust
// Process lifecycle
event_system.on_process_started(|event| {
    println!("Started: {} ({})", event.name, event.pid);
}).await?;

event_system.on_process_terminated(|event| {
    println!("Terminated: {} ({})", event.name, event.pid);
}).await?;

// Resource usage monitoring
event_system.on_process_event(|event| {
    if event.event_type == ProcessEventType::CpuUsageHigh {
        println!("High CPU process: {} using {:.1}%", 
                 event.name, event.cpu_usage.unwrap_or(0.0));
    }
}).await?;
```

### System Resource Events

Monitor system-wide resource usage:

```rust
// CPU monitoring
event_system.on_cpu_usage_high(75.0, |event| {
    println!("CPU usage: {:.1}%", event.cpu_usage.unwrap_or(0.0));
}).await?;

// Memory monitoring
event_system.on_memory_usage_high(80.0, |event| {
    println!("Memory usage: {:.1}%", event.memory_usage.unwrap_or(0.0));
}).await?;

// Comprehensive system monitoring
event_system.on_system_event(|event| {
    match event.event_type {
        SystemEventType::CpuUsageHigh => println!("High CPU usage detected"),
        SystemEventType::MemoryUsageHigh => println!("High memory usage detected"),
        SystemEventType::DiskSpaceLow => println!("Low disk space detected"),
        SystemEventType::TemperatureHigh => println!("High temperature detected"),
        _ => {}
    }
}).await?;
```

### Network Events

Monitor network interface changes and traffic:

```rust
event_system.on_network_event(|event| {
    match event.event_type {
        NetworkEventType::InterfaceUp => {
            println!("Interface {} is now up", event.interface_name.as_ref().unwrap());
        }
        NetworkEventType::InterfaceDown => {
            println!("Interface {} is now down", event.interface_name.as_ref().unwrap());
        }
        NetworkEventType::TrafficThresholdReached => {
            println!("High traffic on {}: {} bytes sent, {} bytes received",
                     event.interface_name.as_ref().unwrap(),
                     event.bytes_sent.unwrap_or(0),
                     event.bytes_received.unwrap_or(0));
        }
        _ => {}
    }
}).await?;
```

### Power Events

Monitor battery and power source changes:

```rust
// Battery monitoring
event_system.on_battery_low(25.0, |event| {
    println!("Battery low: {:.1}%", event.battery_level.unwrap_or(0.0));
}).await?;

// Power state changes
event_system.on_power_event(|event| {
    match event.event_type {
        PowerEventType::BatteryCharging => {
            println!("Battery started charging ({}%)", 
                     event.battery_level.unwrap_or(0.0));
        }
        PowerEventType::BatteryDischarging => {
            println!("Battery stopped charging ({}%)", 
                     event.battery_level.unwrap_or(0.0));
        }
        PowerEventType::PowerSourceChanged => {
            println!("Power source changed to: {:?}", event.power_source);
        }
        _ => {}
    }
}).await?;
```

## Advanced Usage

### Custom Configurations

Configure handlers with custom thresholds and intervals:

```rust
use tell_me_when::{EventSystem, handlers::fs::FsWatchConfig};
use std::time::Duration;

let mut event_system = EventSystem::new();

// Configure file system watcher
let fs_config = FsWatchConfig {
    watch_subdirectories: true,
    ignore_patterns: vec![
        "*.tmp".to_string(),
        ".git/*".to_string(),
        "node_modules/*".to_string(),
    ],
    debounce_events: true,
    event_types: vec![
        FsEventType::Created,
        FsEventType::Modified,
        FsEventType::Deleted,
    ],
    ..Default::default()
};
```

### Event Filtering

Filter events based on custom criteria:

```rust
// Monitor only Rust files
event_system.on_fs_event("./src", |event| {
    let path_str = event.path.to_string_lossy();
    if path_str.ends_with(".rs") {
        println!("Rust file changed: {:?}", event.path);
    }
}).await?;

// Monitor specific processes
event_system.on_process_event(|event| {
    if event.name.contains("cargo") || event.name.contains("rustc") {
        println!("Rust toolchain activity: {}", event.name);
    }
}).await?;
```

### Unsubscribing from Events

Manage event subscriptions dynamically:

```rust
// Subscribe and get event ID
let event_id = event_system.on_fs_created("./", |event| {
    println!("File created: {:?}", event.path);
}).await?;

// Later, unsubscribe
event_system.unsubscribe(event_id).await;
```

## Platform Support

### Windows
- Uses Windows API (`ReadDirectoryChangesW`, `GetSystemPowerStatus`)
- Native file system monitoring with `FILE_NOTIFY_CHANGE_*` flags
- System resource monitoring via WMI and performance counters

### Linux
- Uses `inotify` for file system monitoring
- `/proc` and `/sys` filesystem for system information
- Power monitoring via `/sys/class/power_supply/`

### macOS
- Uses FSEvents API for file system monitoring
- IOKit for power and system information
- Native Cocoa APIs for system resource monitoring

## Performance

Tell Me When is designed for high performance with minimal overhead:

- **Zero-copy event handling** where possible
- **Efficient native API usage** on each platform
- **Configurable polling intervals** to balance responsiveness and resource usage
- **Debouncing support** to reduce event noise
- **Selective monitoring** to avoid unnecessary overhead

## Error Handling

Comprehensive error handling with custom error types:

```rust
use tell_me_when::{EventSystem, TellMeWhenError};

match event_system.on_fs_created("./nonexistent", |_| {}).await {
    Ok(event_id) => println!("Successfully subscribed: {}", event_id),
    Err(TellMeWhenError::Io(e)) => eprintln!("IO error: {}", e),
    Err(TellMeWhenError::HandlerNotFound(name)) => eprintln!("Handler not found: {}", name),
    Err(e) => eprintln!("Other error: {}", e),
}
```

## Examples

Check out the [examples](examples/) directory for more comprehensive usage examples:

- [`basic_usage.rs`](examples/basic_usage.rs) - Basic event monitoring setup
- Run with: `cargo run --example basic_usage`

## Contributing

Contributions are welcome! Please feel free to submit pull requests, create issues, or suggest improvements.

### Development Setup

```bash
git clone https://github.com/tristanpoland/tell_me_when.git
cd tell_me_when
cargo build
cargo test
```

### Adding New Event Types

The library is designed to be easily extensible. To add new event types:

1. Define event types in `src/events.rs`
2. Create a handler in `src/handlers/`
3. Implement the `EventHandler` trait
4. Add methods to `EventSystem`
5. Write tests and documentation

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

## Acknowledgments

- Built with Rust's powerful type system and async/await
- Inspired by various file watching and system monitoring libraries
- Cross-platform compatibility through careful API abstraction
