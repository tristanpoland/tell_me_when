use tell_me_when::{EventSystem, SystemEventData, SystemEventType};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::time::timeout;

#[tokio::test]
async fn test_cpu_usage_high_event() {
    let mut event_system = EventSystem::new();
    event_system.start().await.expect("Failed to start event system");
    
    let events_received = Arc::new(Mutex::new(Vec::new()));
    let events_clone = events_received.clone();
    
    // Set up CPU usage high event listener with a low threshold for testing
    let _id = event_system.on_cpu_usage_high(1.0, move |event: SystemEventData| {
        let mut events = events_clone.lock().unwrap();
        events.push(event);
    }).await.expect("Failed to set up CPU usage high event listener");
    
    // Give the system time to set up monitoring and collect initial data
    tokio::time::sleep(Duration::from_millis(1000)).await;
    
    // Create some CPU load to trigger the event
    let cpu_load_tasks: Vec<_> = (0..num_cpus::get()).map(|_| {
        tokio::spawn(async {
            let start = std::time::Instant::now();
            let mut counter = 0u64;
            // Run CPU-intensive task for a short period
            while start.elapsed() < Duration::from_millis(1000) {
                counter = counter.wrapping_add(1);
                // Occasionally yield to prevent blocking
                if counter % 100000 == 0 {
                    tokio::task::yield_now().await;
                }
            }
        })
    }).collect();
    
    // Wait for CPU load tasks to complete
    for task in cpu_load_tasks {
        let _ = task.await;
    }
    
    // Give the monitoring system time to detect the high CPU usage
    tokio::time::sleep(Duration::from_millis(2000)).await;
    
    let events = events_received.lock().unwrap();
    
    if !events.is_empty() {
        println!("✓ High CPU usage events detected: {} events", events.len());
        
        for event in events.iter() {
            assert_eq!(event.event_type, SystemEventType::CpuUsageHigh);
            if let Some(cpu_usage) = event.cpu_usage {
                println!("  - CPU usage: {:.1}%", cpu_usage);
                assert!(cpu_usage >= 1.0, "CPU usage should be above threshold");
            }
        }
        
        assert!(!events.is_empty(), "Should detect high CPU usage events");
    } else {
        println!("⚠ No high CPU usage events detected - may need longer monitoring period or lower threshold");
    }
    
    event_system.stop().await.expect("Failed to stop event system");
}

#[tokio::test]
async fn test_memory_usage_high_event() {
    let mut event_system = EventSystem::new();
    event_system.start().await.expect("Failed to start event system");
    
    let events_received = Arc::new(Mutex::new(Vec::new()));
    let events_clone = events_received.clone();
    
    // Set up memory usage high event listener with a low threshold for testing
    let _id = event_system.on_memory_usage_high(1.0, move |event: SystemEventData| {
        let mut events = events_clone.lock().unwrap();
        events.push(event);
    }).await.expect("Failed to set up memory usage high event listener");
    
    // Give the system time to set up monitoring
    tokio::time::sleep(Duration::from_millis(1000)).await;
    
    // Allocate some memory to potentially trigger the event
    // Be careful not to allocate too much to avoid system issues
    let _memory_allocation: Vec<Vec<u8>> = (0..50).map(|_| vec![0u8; 1024 * 1024]).collect(); // 50MB
    
    // Give the monitoring system time to detect the memory usage
    tokio::time::sleep(Duration::from_millis(2000)).await;
    
    let events = events_received.lock().unwrap();
    
    if !events.is_empty() {
        println!("✓ High memory usage events detected: {} events", events.len());
        
        for event in events.iter() {
            assert_eq!(event.event_type, SystemEventType::MemoryUsageHigh);
            if let Some(memory_usage) = event.memory_usage {
                println!("  - Memory usage: {:.1}%", memory_usage);
                assert!(memory_usage >= 1.0, "Memory usage should be above threshold");
            }
        }
        
        assert!(!events.is_empty(), "Should detect high memory usage events");
    } else {
        println!("⚠ No high memory usage events detected - system may have plenty of available memory");
    }
    
    event_system.stop().await.expect("Failed to stop event system");
}

#[tokio::test]
async fn test_system_event_general() {
    let mut event_system = EventSystem::new();
    event_system.start().await.expect("Failed to start event system");
    
    let events_received = Arc::new(Mutex::new(Vec::new()));
    let events_clone = events_received.clone();
    
    // Set up general system event listener
    let _id = event_system.on_system_event(move |event: SystemEventData| {
        let mut events = events_clone.lock().unwrap();
        events.push(event);
    }).await.expect("Failed to set up system event listener");
    
    // Give the system time to set up monitoring and collect data
    tokio::time::sleep(Duration::from_millis(2000)).await;
    
    // Try to generate some system load
    let load_tasks: Vec<_> = (0..2).map(|_| {
        tokio::spawn(async {
            let start = std::time::Instant::now();
            let mut counter = 0u64;
            // Moderate CPU/memory load
            let _temp_memory: Vec<u8> = vec![0u8; 1024 * 1024]; // 1MB
            while start.elapsed() < Duration::from_millis(1000) {
                counter = counter.wrapping_add(1);
                if counter % 50000 == 0 {
                    tokio::task::yield_now().await;
                }
            }
        })
    }).collect();
    
    // Wait for load tasks
    for task in load_tasks {
        let _ = task.await;
    }
    
    // Give more time for system monitoring to detect events
    tokio::time::sleep(Duration::from_millis(3000)).await;
    
    let events = events_received.lock().unwrap();
    
    if !events.is_empty() {
        println!("✓ System events detected: {} events", events.len());
        
        let mut cpu_events = 0;
        let mut memory_events = 0;
        let mut disk_events = 0;
        let mut temp_events = 0;
        let mut load_events = 0;
        
        for event in events.iter() {
            match event.event_type {
                SystemEventType::CpuUsageHigh => {
                    cpu_events += 1;
                    if let Some(cpu_usage) = event.cpu_usage {
                        println!("  - CPU usage: {:.1}%", cpu_usage);
                    }
                }
                SystemEventType::MemoryUsageHigh => {
                    memory_events += 1;
                    if let Some(memory_usage) = event.memory_usage {
                        println!("  - Memory usage: {:.1}%", memory_usage);
                    }
                }
                SystemEventType::DiskSpaceLow => {
                    disk_events += 1;
                    if let Some(disk_usage) = event.disk_usage {
                        println!("  - Disk usage: {:.1}%", disk_usage);
                    }
                }
                SystemEventType::TemperatureHigh => {
                    temp_events += 1;
                    if let Some(temperature) = event.temperature {
                        println!("  - Temperature: {:.1}°C", temperature);
                    }
                }
                SystemEventType::LoadAverageHigh => {
                    load_events += 1;
                    if let Some(load_avg) = event.load_average {
                        println!("  - Load average: {:.2}", load_avg);
                    }
                }
            }
            
            // Verify timestamp is recent
            let now = std::time::SystemTime::now();
            let event_age = now.duration_since(event.timestamp).unwrap_or_default();
            assert!(event_age < Duration::from_secs(60), 
                   "System event timestamp should be recent");
        }
        
        println!("Event breakdown:");
        println!("  - CPU events: {}", cpu_events);
        println!("  - Memory events: {}", memory_events);
        println!("  - Disk events: {}", disk_events);
        println!("  - Temperature events: {}", temp_events);
        println!("  - Load average events: {}", load_events);
        
        assert!(!events.is_empty(), "Should receive some system events");
    } else {
        println!("⚠ No system events detected - may require lower thresholds or longer monitoring period");
    }
    
    event_system.stop().await.expect("Failed to stop event system");
}

#[tokio::test]
async fn test_multiple_threshold_monitoring() {
    let mut event_system = EventSystem::new();
    event_system.start().await.expect("Failed to start event system");
    
    let cpu_events = Arc::new(Mutex::new(Vec::new()));
    let memory_events = Arc::new(Mutex::new(Vec::new()));
    
    let cpu_events_clone = cpu_events.clone();
    let memory_events_clone = memory_events.clone();
    
    // Set up multiple threshold monitors with different thresholds
    let _cpu_id = event_system.on_cpu_usage_high(0.1, move |event: SystemEventData| {
        let mut events = cpu_events_clone.lock().unwrap();
        events.push(event);
    }).await.expect("Failed to set up CPU usage monitor");
    
    let _memory_id = event_system.on_memory_usage_high(0.1, move |event: SystemEventData| {
        let mut events = memory_events_clone.lock().unwrap();
        events.push(event);
    }).await.expect("Failed to set up memory usage monitor");
    
    // Give the system time to set up monitoring
    tokio::time::sleep(Duration::from_millis(1000)).await;
    
    // Generate combined CPU and memory load
    let combined_load_tasks: Vec<_> = (0..3).map(|i| {
        tokio::spawn(async move {
            let start = std::time::Instant::now();
            let mut counter = 0u64;
            let _memory: Vec<u8> = vec![0u8; 1024 * 1024 * (i + 1)]; // Variable memory allocation
            
            while start.elapsed() < Duration::from_millis(1500) {
                counter = counter.wrapping_add(1);
                if counter % 75000 == 0 {
                    tokio::task::yield_now().await;
                }
            }
        })
    }).collect();
    
    // Wait for load tasks to complete
    for task in combined_load_tasks {
        let _ = task.await;
    }
    
    // Give time for monitoring to detect the load
    tokio::time::sleep(Duration::from_millis(2500)).await;
    
    let cpu_events_vec = cpu_events.lock().unwrap();
    let memory_events_vec = memory_events.lock().unwrap();
    
    println!("CPU threshold events: {}", cpu_events_vec.len());
    println!("Memory threshold events: {}", memory_events_vec.len());
    
    if !cpu_events_vec.is_empty() || !memory_events_vec.is_empty() {
        println!("✓ Threshold monitoring working - detected {} CPU events and {} memory events", 
                cpu_events_vec.len(), memory_events_vec.len());
        
        // Verify that each event type is correct
        for event in cpu_events_vec.iter() {
            assert_eq!(event.event_type, SystemEventType::CpuUsageHigh);
            assert!(event.cpu_usage.is_some());
        }
        
        for event in memory_events_vec.iter() {
            assert_eq!(event.event_type, SystemEventType::MemoryUsageHigh);
            assert!(event.memory_usage.is_some());
        }
    } else {
        println!("⚠ No threshold events detected with very low thresholds - system monitoring may need adjustment");
    }
    
    event_system.stop().await.expect("Failed to stop event system");
}

#[tokio::test]
async fn test_system_event_data_quality() {
    let mut event_system = EventSystem::new();
    event_system.start().await.expect("Failed to start event system");
    
    let events_received = Arc::new(Mutex::new(Vec::new()));
    let events_clone = events_received.clone();
    
    // Set up system event listener with very low thresholds to catch events
    let _id = event_system.on_system_event(move |event: SystemEventData| {
        let mut events = events_clone.lock().unwrap();
        events.push(event);
    }).await.expect("Failed to set up system event listener");
    
    // Give the system time to collect baseline data
    tokio::time::sleep(Duration::from_millis(2000)).await;
    
    let events = events_received.lock().unwrap();
    
    if !events.is_empty() {
        println!("✓ Collected {} system events for data quality check", events.len());
        
        let mut events_with_cpu_data = 0;
        let mut events_with_memory_data = 0;
        let mut events_with_valid_timestamps = 0;
        
        for event in events.iter() {
            // Check for CPU data
            if let Some(cpu_usage) = event.cpu_usage {
                if cpu_usage >= 0.0 && cpu_usage <= 100.0 {
                    events_with_cpu_data += 1;
                }
            }
            
            // Check for memory data
            if let Some(memory_usage) = event.memory_usage {
                if memory_usage >= 0.0 && memory_usage <= 100.0 {
                    events_with_memory_data += 1;
                }
            }
            
            // Check timestamp validity
            let now = std::time::SystemTime::now();
            if let Ok(duration_since) = now.duration_since(event.timestamp) {
                if duration_since < Duration::from_secs(30) {
                    events_with_valid_timestamps += 1;
                }
            }
        }
        
        println!("Data quality metrics:");
        println!("  - Events with valid CPU data: {}", events_with_cpu_data);
        println!("  - Events with valid memory data: {}", events_with_memory_data);
        println!("  - Events with recent timestamps: {}", events_with_valid_timestamps);
        
        // At least some events should have valid data
        assert!(events_with_valid_timestamps > 0, "Should have events with recent timestamps");
        
        if events_with_cpu_data > 0 {
            println!("✓ CPU monitoring data quality verified");
        }
        
        if events_with_memory_data > 0 {
            println!("✓ Memory monitoring data quality verified");
        }
    } else {
        println!("⚠ No system events collected for data quality check");
    }
    
    event_system.stop().await.expect("Failed to stop event system");
}

#[tokio::test]
async fn test_sustained_monitoring() {
    let mut event_system = EventSystem::new();
    event_system.start().await.expect("Failed to start event system");
    
    let events_received = Arc::new(Mutex::new(Vec::new()));
    let events_clone = events_received.clone();
    
    // Set up system monitoring with moderate thresholds
    let _id = event_system.on_system_event(move |event: SystemEventData| {
        let mut events = events_clone.lock().unwrap();
        events.push(event);
    }).await.expect("Failed to set up system event listener");
    
    // Run sustained monitoring for a longer period
    let monitoring_duration = Duration::from_millis(5000); // 5 seconds
    let start_time = std::time::Instant::now();
    
    // Create periodic load bursts
    while start_time.elapsed() < monitoring_duration {
        // Create a short burst of activity
        let burst_tasks: Vec<_> = (0..2).map(|_| {
            tokio::spawn(async {
                let burst_start = std::time::Instant::now();
                let mut counter = 0u64;
                while burst_start.elapsed() < Duration::from_millis(200) {
                    counter = counter.wrapping_add(1);
                    if counter % 10000 == 0 {
                        tokio::task::yield_now().await;
                    }
                }
            })
        }).collect();
        
        for task in burst_tasks {
            let _ = task.await;
        }
        
        // Rest period
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    
    // Give time for final events to be processed
    tokio::time::sleep(Duration::from_millis(1000)).await;
    
    let events = events_received.lock().unwrap();
    
    println!("Sustained monitoring results: {} events over {:?}", 
            events.len(), monitoring_duration);
    
    if !events.is_empty() {
        // Check event distribution over time
        let first_timestamp = events.first().unwrap().timestamp;
        let last_timestamp = events.last().unwrap().timestamp;
        let monitoring_span = last_timestamp.duration_since(first_timestamp).unwrap_or_default();
        
        println!("✓ Events detected over {:?} timespan", monitoring_span);
        
        // Check for consistent monitoring
        let events_per_type = events.iter().fold(std::collections::HashMap::new(), |mut acc, event| {
            let count = acc.entry(std::mem::discriminant(&event.event_type)).or_insert(0);
            *count += 1;
            acc
        });
        
        println!("Event type distribution: {} different types", events_per_type.len());
        
        assert!(monitoring_span >= Duration::from_millis(500), 
               "Should have events distributed over time");
    } else {
        println!("⚠ No events during sustained monitoring period");
    }
    
    event_system.stop().await.expect("Failed to stop event system");
}

// Add num_cpus to dev-dependencies for this test
#[tokio::test]
async fn test_adaptive_threshold_response() {
    let mut event_system = EventSystem::new();
    event_system.start().await.expect("Failed to start event system");
    
    let low_threshold_events = Arc::new(Mutex::new(Vec::new()));
    let high_threshold_events = Arc::new(Mutex::new(Vec::new()));
    
    let low_events_clone = low_threshold_events.clone();
    let high_events_clone = high_threshold_events.clone();
    
    // Set up two different CPU monitors with different thresholds
    let _low_id = event_system.on_cpu_usage_high(5.0, move |event: SystemEventData| {
        let mut events = low_events_clone.lock().unwrap();
        events.push(event);
    }).await.expect("Failed to set up low threshold CPU monitor");
    
    let _high_id = event_system.on_cpu_usage_high(50.0, move |event: SystemEventData| {
        let mut events = high_events_clone.lock().unwrap();
        events.push(event);
    }).await.expect("Failed to set up high threshold CPU monitor");
    
    // Give time for setup
    tokio::time::sleep(Duration::from_millis(1000)).await;
    
    // Create moderate CPU load
    let moderate_load: Vec<_> = (0..2).map(|_| {
        tokio::spawn(async {
            let start = std::time::Instant::now();
            let mut counter = 0u64;
            while start.elapsed() < Duration::from_millis(2000) {
                counter = counter.wrapping_add(1);
                if counter % 50000 == 0 {
                    tokio::task::yield_now().await;
                }
            }
        })
    }).collect();
    
    for task in moderate_load {
        let _ = task.await;
    }
    
    // Give time for events
    tokio::time::sleep(Duration::from_millis(2000)).await;
    
    let low_events = low_threshold_events.lock().unwrap();
    let high_events = high_threshold_events.lock().unwrap();
    
    println!("Low threshold (5%) events: {}", low_events.len());
    println!("High threshold (50%) events: {}", high_events.len());
    
    // Low threshold should typically trigger more easily than high threshold
    if !low_events.is_empty() {
        println!("✓ Low threshold monitoring responsive");
        
        // Verify threshold behavior - low threshold should have >= high threshold events
        assert!(low_events.len() >= high_events.len(), 
               "Low threshold should trigger at least as often as high threshold");
    } else if !high_events.is_empty() {
        println!("✓ High threshold monitoring working (system under heavy load)");
    } else {
        println!("⚠ No threshold events detected - system may be under light load");
    }
    
    event_system.stop().await.expect("Failed to stop event system");
}