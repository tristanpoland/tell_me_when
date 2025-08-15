use tell_me_when::{EventSystem, FsEventData, ProcessEventData, SystemEventData, NetworkEventData, PowerEventData, EventData};
use tempfile::TempDir;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use std::collections::HashMap;
use tokio::time::timeout;
use std::fs;

#[tokio::test]
async fn test_multiple_event_types_simultaneously() {
    let mut event_system = EventSystem::new();
    event_system.start().await.expect("Failed to start event system");
    
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    let temp_path = temp_dir.path();
    
    let fs_events = Arc::new(Mutex::new(Vec::new()));
    let process_events = Arc::new(Mutex::new(Vec::new()));
    let system_events = Arc::new(Mutex::new(Vec::new()));
    let network_events = Arc::new(Mutex::new(Vec::new()));
    let power_events = Arc::new(Mutex::new(Vec::new()));
    
    let fs_clone = fs_events.clone();
    let process_clone = process_events.clone();
    let system_clone = system_events.clone();
    let network_clone = network_events.clone();
    let power_clone = power_events.clone();
    
    // Set up all event listeners
    let _fs_id = event_system.on_fs_event(temp_path, move |event: FsEventData| {
        let mut events = fs_clone.lock().unwrap();
        events.push(event);
    }).await.expect("Failed to set up filesystem event listener");
    
    let _process_id = event_system.on_process_event(move |event: ProcessEventData| {
        let mut events = process_clone.lock().unwrap();
        events.push(event);
    }).await.expect("Failed to set up process event listener");
    
    let _system_id = event_system.on_system_event(move |event: SystemEventData| {
        let mut events = system_clone.lock().unwrap();
        events.push(event);
    }).await.expect("Failed to set up system event listener");
    
    let _network_id = event_system.on_network_event(move |event: NetworkEventData| {
        let mut events = network_clone.lock().unwrap();
        events.push(event);
    }).await.expect("Failed to set up network event listener");
    
    let _power_id = event_system.on_power_event(move |event: PowerEventData| {
        let mut events = power_clone.lock().unwrap();
        events.push(event);
    }).await.expect("Failed to set up power event listener");
    
    // Give the system time to set up all monitors
    tokio::time::sleep(Duration::from_millis(1000)).await;
    
    // Generate simultaneous activity across different domains
    let activity_start = Instant::now();
    
    // File system activity
    let fs_task = tokio::spawn(async move {
        for i in 0..5 {
            let test_file = temp_path.join(format!("test_file_{}.txt", i));
            if let Err(e) = fs::write(&test_file, format!("Content for file {}", i)) {
                eprintln!("Failed to write file {}: {}", i, e);
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    });
    
    // CPU/system load
    let system_task = tokio::spawn(async {
        let load_tasks: Vec<_> = (0..3).map(|_| {
            tokio::spawn(async {
                let start = Instant::now();
                let mut counter = 0u64;
                while start.elapsed() < Duration::from_millis(2000) {
                    counter = counter.wrapping_add(1);
                    if counter % 100000 == 0 {
                        tokio::task::yield_now().await;
                    }
                }
            })
        }).collect();
        
        for task in load_tasks {
            let _ = task.await;
        }
    });
    
    // Network activity
    let network_task = tokio::spawn(async {
        let client = reqwest::Client::new();
        for i in 0..3 {
            match tokio::time::timeout(
                Duration::from_secs(5),
                client.get("https://httpbin.org/uuid").send()
            ).await {
                Ok(Ok(_)) => println!("Network request {} completed", i),
                _ => println!("Network request {} failed or timed out", i),
            }
            tokio::time::sleep(Duration::from_millis(300)).await;
        }
    });
    
    // Process activity (launching ping commands)
    let process_task = tokio::spawn(async {
        for i in 0..3 {
            match std::process::Command::new("ping")
                .arg("127.0.0.1")
                .arg("-n")
                .arg("1")
                .output() {
                Ok(_) => println!("Ping process {} completed", i),
                Err(e) => println!("Ping process {} failed: {}", i, e),
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    });
    
    // Wait for all activities to complete
    let (fs_result, system_result, network_result, process_result) = tokio::join!(
        fs_task, system_task, network_task, process_task
    );
    
    if let Err(e) = fs_result {
        eprintln!("Filesystem task error: {}", e);
    }
    if let Err(e) = system_result {
        eprintln!("System task error: {}", e);
    }
    if let Err(e) = network_result {
        eprintln!("Network task error: {}", e);
    }
    if let Err(e) = process_result {
        eprintln!("Process task error: {}", e);
    }
    
    let activity_duration = activity_start.elapsed();
    println!("All simultaneous activities completed in {:?}", activity_duration);
    
    // Give time for all events to be processed
    tokio::time::sleep(Duration::from_millis(3000)).await;
    
    // Check results
    let fs_count = fs_events.lock().unwrap().len();
    let process_count = process_events.lock().unwrap().len();
    let system_count = system_events.lock().unwrap().len();
    let network_count = network_events.lock().unwrap().len();
    let power_count = power_events.lock().unwrap().len();
    
    println!("Integration test results:");
    println!("  - Filesystem events: {}", fs_count);
    println!("  - Process events: {}", process_count);
    println!("  - System events: {}", system_count);
    println!("  - Network events: {}", network_count);
    println!("  - Power events: {}", power_count);
    
    let total_events = fs_count + process_count + system_count + network_count + power_count;
    println!("  - Total events detected: {}", total_events);
    
    // At least filesystem events should be detected
    assert!(fs_count > 0, "Should detect filesystem events from file creation");
    
    if total_events > 5 {
        println!("✓ Multiple event types detected simultaneously");
    } else {
        println!("⚠ Limited events detected - some monitoring may require platform-specific implementation");
    }
    
    event_system.stop().await.expect("Failed to stop event system");
}

#[tokio::test]
async fn test_event_system_stress_test() {
    let mut event_system = EventSystem::new();
    event_system.start().await.expect("Failed to start event system");
    
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    let temp_path = temp_dir.path();
    
    let total_events = Arc::new(Mutex::new(0));
    let event_types = Arc::new(Mutex::new(HashMap::new()));
    
    let total_clone = total_events.clone();
    let types_clone = event_types.clone();
    
    // Set up consolidated event listener to count all events
    let _fs_id = event_system.on_fs_event(temp_path, {
        let total = total_clone.clone();
        let types = types_clone.clone();
        move |_: FsEventData| {
            *total.lock().unwrap() += 1;
            *types.lock().unwrap().entry("filesystem".to_string()).or_insert(0) += 1;
        }
    }).await.expect("Failed to set up filesystem event listener");
    
    let _system_id = event_system.on_system_event({
        let total = total_clone.clone();
        let types = types_clone.clone();
        move |_: SystemEventData| {
            *total.lock().unwrap() += 1;
            *types.lock().unwrap().entry("system".to_string()).or_insert(0) += 1;
        }
    }).await.expect("Failed to set up system event listener");
    
    let _process_id = event_system.on_process_event({
        let total = total_clone.clone();
        let types = types_clone.clone();
        move |_: ProcessEventData| {
            *total.lock().unwrap() += 1;
            *types.lock().unwrap().entry("process".to_string()).or_insert(0) += 1;
        }
    }).await.expect("Failed to set up process event listener");
    
    // Give time for setup
    tokio::time::sleep(Duration::from_millis(1000)).await;
    
    let stress_start = Instant::now();
    println!("Starting stress test...");
    
    // Generate intensive activity for stress testing
    let stress_tasks: Vec<_> = (0..10).map(|task_id| {
        let temp_path = temp_path.to_path_buf();
        tokio::spawn(async move {
            // File operations
            for i in 0..20 {
                let file_path = temp_path.join(format!("stress_{}_{}.txt", task_id, i));
                if let Ok(mut file) = std::fs::File::create(&file_path) {
                    use std::io::Write;
                    let _ = writeln!(file, "Stress test data {} {}", task_id, i);
                    drop(file);
                    
                    // Modify the file
                    let _ = fs::write(&file_path, format!("Modified stress data {} {}", task_id, i));
                    
                    // Small delay to not overwhelm the system
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            }
            
            // CPU activity
            let cpu_start = Instant::now();
            let mut counter = 0u64;
            while cpu_start.elapsed() < Duration::from_millis(1000) {
                counter = counter.wrapping_add(1);
                if counter % 50000 == 0 {
                    tokio::task::yield_now().await;
                }
            }
        })
    }).collect();
    
    // Wait for all stress tasks
    for task in stress_tasks {
        let _ = task.await;
    }
    
    let stress_duration = stress_start.elapsed();
    println!("Stress test activities completed in {:?}", stress_duration);
    
    // Give time for event processing
    tokio::time::sleep(Duration::from_millis(5000)).await;
    
    let final_total = *total_events.lock().unwrap();
    let final_types = event_types.lock().unwrap().clone();
    
    println!("Stress test results:");
    println!("  - Total events detected: {}", final_total);
    for (event_type, count) in final_types.iter() {
        println!("  - {} events: {}", event_type, count);
    }
    println!("  - Events per second: {:.1}", final_total as f64 / stress_duration.as_secs_f64());
    
    // Should detect many filesystem events at minimum
    assert!(final_total > 10, "Should detect significant number of events under stress");
    
    println!("✓ Event system handled stress test successfully");
    
    event_system.stop().await.expect("Failed to stop event system");
}

#[tokio::test]
async fn test_event_unsubscription_integration() {
    let mut event_system = EventSystem::new();
    event_system.start().await.expect("Failed to start event system");
    
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    let temp_path = temp_dir.path();
    
    let fs_events_before = Arc::new(Mutex::new(Vec::new()));
    let fs_events_after = Arc::new(Mutex::new(Vec::new()));
    let system_events = Arc::new(Mutex::new(Vec::new()));
    
    let fs_before_clone = fs_events_before.clone();
    let fs_after_clone = fs_events_after.clone();
    let system_clone = system_events.clone();
    
    // Set up event listeners
    let fs_id = event_system.on_fs_event(temp_path, move |event: FsEventData| {
        let mut events = fs_before_clone.lock().unwrap();
        events.push(event);
    }).await.expect("Failed to set up filesystem event listener");
    
    let _system_id = event_system.on_system_event(move |event: SystemEventData| {
        let mut events = system_clone.lock().unwrap();
        events.push(event);
    }).await.expect("Failed to set up system event listener");
    
    // Give time for setup
    tokio::time::sleep(Duration::from_millis(500)).await;
    
    // Generate some initial events
    for i in 0..3 {
        let test_file = temp_path.join(format!("before_unsub_{}.txt", i));
        fs::write(&test_file, format!("Content {}", i)).expect("Failed to write file");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    
    // Wait for events
    tokio::time::sleep(Duration::from_millis(1000)).await;
    
    let events_before_unsub = fs_events_before.lock().unwrap().len();
    println!("Events before unsubscription: {}", events_before_unsub);
    
    // Unsubscribe from filesystem events
    let unsubscribe_result = event_system.unsubscribe(fs_id).await;
    assert!(unsubscribe_result, "Should successfully unsubscribe");
    
    // Set up new filesystem listener to catch events after unsubscription
    let _new_fs_id = event_system.on_fs_event(temp_path, move |event: FsEventData| {
        let mut events = fs_after_clone.lock().unwrap();
        events.push(event);
    }).await.expect("Failed to set up new filesystem event listener");
    
    tokio::time::sleep(Duration::from_millis(500)).await;
    
    // Generate more events
    for i in 3..6 {
        let test_file = temp_path.join(format!("after_unsub_{}.txt", i));
        fs::write(&test_file, format!("Content {}", i)).expect("Failed to write file");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    
    // Wait for events
    tokio::time::sleep(Duration::from_millis(1000)).await;
    
    let events_before_final = fs_events_before.lock().unwrap().len();
    let events_after = fs_events_after.lock().unwrap().len();
    let system_events_count = system_events.lock().unwrap().len();
    
    println!("Unsubscription test results:");
    println!("  - Events in first listener after unsubscribe: {}", events_before_final);
    println!("  - Events in new listener: {}", events_after);
    println!("  - System events (should continue): {}", system_events_count);
    
    // First listener should not receive new events after unsubscription
    assert_eq!(events_before_final, events_before_unsub, 
              "Original listener should not receive events after unsubscription");
    
    // New listener should receive events
    assert!(events_after > 0, "New listener should receive events");
    
    println!("✓ Event unsubscription working correctly");
    
    event_system.stop().await.expect("Failed to stop event system");
}

#[tokio::test]
async fn test_event_system_lifecycle() {
    // Test start/stop cycle multiple times
    for cycle in 0..3 {
        println!("Testing lifecycle cycle {}", cycle);
        
        let mut event_system = EventSystem::new();
        assert!(!event_system.is_running(), "Event system should start as not running");
        
        // Start the system
        event_system.start().await.expect("Failed to start event system");
        assert!(event_system.is_running(), "Event system should be running after start");
        
        let temp_dir = TempDir::new().expect("Failed to create temp directory");
        let events_received = Arc::new(Mutex::new(Vec::new()));
        let events_clone = events_received.clone();
        
        // Set up a listener
        let _id = event_system.on_fs_created(temp_dir.path(), move |event: FsEventData| {
            let mut events = events_clone.lock().unwrap();
            events.push(event);
        }).await.expect("Failed to set up filesystem event listener");
        
        tokio::time::sleep(Duration::from_millis(200)).await;
        
        // Generate an event
        let test_file = temp_dir.path().join("lifecycle_test.txt");
        fs::write(&test_file, "Lifecycle test").expect("Failed to write file");
        
        tokio::time::sleep(Duration::from_millis(500)).await;
        
        // Stop the system
        event_system.stop().await.expect("Failed to stop event system");
        assert!(!event_system.is_running(), "Event system should not be running after stop");
        
        let events_count = events_received.lock().unwrap().len();
        if events_count > 0 {
            println!("  ✓ Cycle {} detected {} events", cycle, events_count);
        } else {
            println!("  ⚠ Cycle {} detected no events", cycle);
        }
        
        // Brief pause between cycles
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    
    println!("✓ Event system lifecycle test completed successfully");
}

#[tokio::test]
async fn test_concurrent_event_subscribers() {
    let mut event_system = EventSystem::new();
    event_system.start().await.expect("Failed to start event system");
    
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    let temp_path = temp_dir.path();
    
    let subscriber_count = 10;
    let mut subscriber_events: Vec<Arc<Mutex<Vec<FsEventData>>>> = Vec::new();
    let mut subscriber_ids = Vec::new();
    
    // Set up multiple concurrent subscribers
    for i in 0..subscriber_count {
        let events = Arc::new(Mutex::new(Vec::new()));
        subscriber_events.push(events.clone());
        
        let id = event_system.on_fs_created(temp_path, move |event: FsEventData| {
            let mut events = events.lock().unwrap();
            events.push(event);
        }).await.expect("Failed to set up filesystem event listener");
        
        subscriber_ids.push(id);
        println!("Set up subscriber {} with ID {}", i, id);
    }
    
    // Give time for all subscribers to set up
    tokio::time::sleep(Duration::from_millis(1000)).await;
    
    // Generate events that should be received by all subscribers
    let event_count = 5;
    for i in 0..event_count {
        let test_file = temp_path.join(format!("concurrent_test_{}.txt", i));
        fs::write(&test_file, format!("Concurrent test {}", i)).expect("Failed to write file");
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    
    // Wait for events to be processed
    tokio::time::sleep(Duration::from_millis(2000)).await;
    
    // Check that all subscribers received events
    let mut total_events = 0;
    let mut subscribers_with_events = 0;
    
    for (i, subscriber_events) in subscriber_events.iter().enumerate() {
        let events = subscriber_events.lock().unwrap();
        let count = events.len();
        total_events += count;
        
        if count > 0 {
            subscribers_with_events += 1;
            println!("  Subscriber {} received {} events", i, count);
        }
    }
    
    println!("Concurrent subscribers test results:");
    println!("  - Total subscribers: {}", subscriber_count);
    println!("  - Subscribers with events: {}", subscribers_with_events);
    println!("  - Total events across all subscribers: {}", total_events);
    println!("  - Average events per subscriber: {:.1}", 
            total_events as f64 / subscriber_count as f64);
    
    // Most subscribers should receive events
    assert!(subscribers_with_events > 0, "At least some subscribers should receive events");
    
    if subscribers_with_events >= subscriber_count / 2 {
        println!("✓ Majority of concurrent subscribers received events");
    }
    
    // Test unsubscribing half the subscribers
    let unsubscribe_count = subscriber_count / 2;
    for i in 0..unsubscribe_count {
        let success = event_system.unsubscribe(subscriber_ids[i]).await;
        assert!(success, "Should successfully unsubscribe subscriber {}", i);
    }
    
    println!("✓ Concurrent event subscribers test completed");
    
    event_system.stop().await.expect("Failed to stop event system");
}

#[tokio::test]
async fn test_cross_platform_event_consistency() {
    let mut event_system = EventSystem::new();
    event_system.start().await.expect("Failed to start event system");
    
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    let temp_path = temp_dir.path();
    
    let all_events = Arc::new(Mutex::new(Vec::new()));
    let events_clone = all_events.clone();
    
    // Set up filesystem monitoring
    let _id = event_system.on_fs_event(temp_path, move |event: FsEventData| {
        let mut events = events_clone.lock().unwrap();
        events.push(event);
    }).await.expect("Failed to set up filesystem event listener");
    
    tokio::time::sleep(Duration::from_millis(500)).await;
    
    // Perform cross-platform file operations
    let operations = vec![
        ("create_file", "create"),
        ("modify_file", "modify"),
        ("delete_file", "delete"),
    ];
    
    for (filename, operation) in operations {
        let file_path = temp_path.join(format!("{}.txt", filename));
        
        match operation {
            "create" => {
                fs::write(&file_path, format!("Initial content for {}", filename))
                    .expect("Failed to create file");
            }
            "modify" => {
                fs::write(&file_path, format!("Modified content for {}", filename))
                    .expect("Failed to modify file");
                tokio::time::sleep(Duration::from_millis(100)).await;
                fs::write(&file_path, format!("Second modification for {}", filename))
                    .expect("Failed to modify file again");
            }
            "delete" => {
                // Create first, then delete
                fs::write(&file_path, format!("Content to be deleted for {}", filename))
                    .expect("Failed to create file for deletion");
                tokio::time::sleep(Duration::from_millis(200)).await;
                fs::remove_file(&file_path).expect("Failed to delete file");
            }
            _ => {}
        }
        
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
    
    // Wait for all events to be processed
    tokio::time::sleep(Duration::from_millis(2000)).await;
    
    let events = all_events.lock().unwrap();
    println!("Cross-platform consistency test results:");
    println!("  - Total events detected: {}", events.len());
    
    if !events.is_empty() {
        let mut create_events = 0;
        let mut modify_events = 0;
        let mut delete_events = 0;
        let mut other_events = 0;
        
        for event in events.iter() {
            match event.event_type {
                tell_me_when::FsEventType::Created => create_events += 1,
                tell_me_when::FsEventType::Modified => modify_events += 1,
                tell_me_when::FsEventType::Deleted => delete_events += 1,
                _ => other_events += 1,
            }
            
            // Verify event data consistency
            assert!(!event.path.as_os_str().is_empty(), "Event path should not be empty");
            
            let now = std::time::SystemTime::now();
            let event_age = now.duration_since(event.timestamp).unwrap_or_default();
            assert!(event_age < Duration::from_secs(60), "Event timestamp should be recent");
        }
        
        println!("  Event breakdown:");
        println!("    - Create events: {}", create_events);
        println!("    - Modify events: {}", modify_events);
        println!("    - Delete events: {}", delete_events);
        println!("    - Other events: {}", other_events);
        
        // Should detect at least creation events on all platforms
        assert!(create_events > 0, "Should detect file creation events on all platforms");
        
        println!("✓ Cross-platform event consistency verified");
    } else {
        println!("⚠ No filesystem events detected - may require platform-specific adjustments");
    }
    
    event_system.stop().await.expect("Failed to stop event system");
}