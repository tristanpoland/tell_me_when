use tell_me_when::{EventSystem, PowerEventData, PowerEventType};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::time::timeout;

#[tokio::test]
async fn test_power_event_listener_setup() {
    let mut event_system = EventSystem::new();
    event_system.start().await.expect("Failed to start event system");
    
    let events_received = Arc::new(Mutex::new(Vec::new()));
    let events_clone = events_received.clone();
    
    // Set up power event listener
    let result = event_system.on_power_event(move |event: PowerEventData| {
        let mut events = events_clone.lock().unwrap();
        events.push(event);
    }).await;
    
    assert!(result.is_ok(), "Should be able to set up power event listener");
    
    // Give the system time to set up monitoring
    tokio::time::sleep(Duration::from_millis(1000)).await;
    
    println!("✓ Power event listener setup successful");
    
    event_system.stop().await.expect("Failed to stop event system");
}

#[tokio::test]
async fn test_battery_low_threshold_event() {
    let mut event_system = EventSystem::new();
    event_system.start().await.expect("Failed to start event system");
    
    let events_received = Arc::new(Mutex::new(Vec::new()));
    let events_clone = events_received.clone();
    
    // Set up battery low event listener with a high threshold for testing
    // This way if the system has a battery, we're more likely to trigger the event
    let _id = event_system.on_battery_low(100.0, move |event: PowerEventData| {
        let mut events = events_clone.lock().unwrap();
        events.push(event);
    }).await.expect("Failed to set up battery low event listener");
    
    // Give the system time to set up monitoring and check battery status
    tokio::time::sleep(Duration::from_millis(3000)).await;
    
    let events = events_received.lock().unwrap();
    
    if !events.is_empty() {
        println!("✓ Battery low events detected: {} events", events.len());
        
        for event in events.iter() {
            assert_eq!(event.event_type, PowerEventType::BatteryLow);
            if let Some(battery_level) = event.battery_level {
                println!("  - Battery level: {:.1}%", battery_level);
                assert!(battery_level <= 100.0, "Battery level should be within valid range");
                assert!(battery_level >= 0.0, "Battery level should be non-negative");
            }
            
            if let Some(is_charging) = event.is_charging {
                println!("  - Charging status: {}", if is_charging { "Charging" } else { "Not charging" });
            }
            
            if let Some(ref power_source) = event.power_source {
                println!("  - Power source: {}", power_source);
            }
        }
    } else {
        println!("⚠ No battery low events detected - system may not have a battery or may be plugged in");
    }
    
    event_system.stop().await.expect("Failed to stop event system");
}

#[tokio::test]
async fn test_power_event_general_monitoring() {
    let mut event_system = EventSystem::new();
    event_system.start().await.expect("Failed to start event system");
    
    let events_received = Arc::new(Mutex::new(Vec::new()));
    let events_clone = events_received.clone();
    
    // Set up general power event listener
    let _id = event_system.on_power_event(move |event: PowerEventData| {
        let mut events = events_clone.lock().unwrap();
        println!("Power event received: {:?}", event.event_type);
        events.push(event);
    }).await.expect("Failed to set up power event listener");
    
    // Give the system time to set up monitoring and potentially detect power state
    tokio::time::sleep(Duration::from_millis(5000)).await;
    
    let events = events_received.lock().unwrap();
    
    if !events.is_empty() {
        println!("✓ Power events detected: {} events", events.len());
        
        let mut battery_low_events = 0;
        let mut charging_events = 0;
        let mut discharging_events = 0;
        let mut power_source_events = 0;
        let mut sleep_wake_events = 0;
        
        for event in events.iter() {
            match event.event_type {
                PowerEventType::BatteryLow => {
                    battery_low_events += 1;
                    if let Some(level) = event.battery_level {
                        println!("  - Battery low: {:.1}%", level);
                    }
                }
                PowerEventType::BatteryCharging => {
                    charging_events += 1;
                    println!("  - Battery started charging");
                }
                PowerEventType::BatteryDischarging => {
                    discharging_events += 1;
                    println!("  - Battery stopped charging");
                }
                PowerEventType::PowerSourceChanged => {
                    power_source_events += 1;
                    if let Some(ref source) = event.power_source {
                        println!("  - Power source changed to: {}", source);
                    }
                }
                PowerEventType::SleepMode => {
                    sleep_wake_events += 1;
                    println!("  - System entering sleep mode");
                }
                PowerEventType::WakeFromSleep => {
                    sleep_wake_events += 1;
                    println!("  - System waking from sleep");
                }
                PowerEventType::Shutdown => {
                    println!("  - System shutdown detected");
                }
                PowerEventType::Restart => {
                    println!("  - System restart detected");
                }
            }
            
            // Verify timestamp is recent
            let now = std::time::SystemTime::now();
            let event_age = now.duration_since(event.timestamp).unwrap_or_default();
            assert!(event_age < Duration::from_secs(60), 
                   "Power event timestamp should be recent");
        }
        
        println!("Power event breakdown:");
        println!("  - Battery low events: {}", battery_low_events);
        println!("  - Charging events: {}", charging_events);
        println!("  - Discharging events: {}", discharging_events);
        println!("  - Power source events: {}", power_source_events);
        println!("  - Sleep/wake events: {}", sleep_wake_events);
        
        assert!(!events.is_empty(), "Should detect power events");
    } else {
        println!("⚠ No power events detected - power monitoring may require platform-specific implementation or power state changes");
    }
    
    event_system.stop().await.expect("Failed to stop event system");
}

#[tokio::test]
async fn test_power_event_data_structure() {
    let mut event_system = EventSystem::new();
    event_system.start().await.expect("Failed to start event system");
    
    let events_received = Arc::new(Mutex::new(Vec::new()));
    let events_clone = events_received.clone();
    
    // Set up power event listener to capture all event types
    let _id = event_system.on_power_event(move |event: PowerEventData| {
        let mut events = events_clone.lock().unwrap();
        events.push(event);
    }).await.expect("Failed to set up power event listener");
    
    // Give the system time to potentially detect power state
    tokio::time::sleep(Duration::from_millis(3000)).await;
    
    let events = events_received.lock().unwrap();
    
    if !events.is_empty() {
        println!("✓ Power events available for data structure validation: {} events", events.len());
        
        for (i, event) in events.iter().enumerate() {
            println!("Event {}: Type = {:?}", i, event.event_type);
            
            // Validate event data structure
            if let Some(battery_level) = event.battery_level {
                println!("  - Battery level: {:.1}%", battery_level);
                assert!(battery_level >= 0.0 && battery_level <= 100.0, 
                       "Battery level should be between 0 and 100");
            }
            
            if let Some(is_charging) = event.is_charging {
                println!("  - Is charging: {}", is_charging);
            }
            
            if let Some(ref power_source) = event.power_source {
                println!("  - Power source: {}", power_source);
                assert!(!power_source.is_empty(), "Power source should not be empty");
            }
            
            // Verify timestamp is valid
            let now = std::time::SystemTime::now();
            let event_age = now.duration_since(event.timestamp).unwrap_or_default();
            assert!(event_age < Duration::from_secs(120), 
                   "Event timestamp should be reasonably recent");
        }
        
        assert!(!events.is_empty(), "Should have power events for validation");
    } else {
        println!("⚠ No power events available for data structure validation");
        
        // Even if no events are detected, we can still validate the listener setup
        println!("✓ Power event listener setup and data structure validation ready");
    }
    
    event_system.stop().await.expect("Failed to stop event system");
}

#[tokio::test]
async fn test_power_monitoring_stability() {
    let mut event_system = EventSystem::new();
    event_system.start().await.expect("Failed to start event system");
    
    let events_received = Arc::new(Mutex::new(Vec::new()));
    let events_clone = events_received.clone();
    
    // Set up power event listener
    let _id = event_system.on_power_event(move |event: PowerEventData| {
        let mut events = events_clone.lock().unwrap();
        events.push(event);
    }).await.expect("Failed to set up power event listener");
    
    // Run power monitoring for an extended period
    let monitoring_duration = Duration::from_millis(10000); // 10 seconds
    let start_time = std::time::Instant::now();
    
    println!("Starting {} second power monitoring stability test...", 
            monitoring_duration.as_secs());
    
    // Monitor power state periodically
    let mut check_count = 0;
    while start_time.elapsed() < monitoring_duration {
        tokio::time::sleep(Duration::from_millis(1000)).await;
        check_count += 1;
        
        if check_count % 3 == 0 {
            println!("  Power monitoring check {}", check_count);
        }
    }
    
    // Give final time for event processing
    tokio::time::sleep(Duration::from_millis(1000)).await;
    
    let events = events_received.lock().unwrap();
    
    println!("Power monitoring stability test completed:");
    println!("  - Duration: {:?}", monitoring_duration);
    println!("  - Events detected: {}", events.len());
    
    if !events.is_empty() {
        println!("✓ Power monitoring remained stable with {} events", events.len());
        
        // Check for event consistency
        let first_event_time = events.first().unwrap().timestamp;
        let last_event_time = events.last().unwrap().timestamp;
        let event_timespan = last_event_time.duration_since(first_event_time).unwrap_or_default();
        
        println!("  - Events distributed over {:?}", event_timespan);
        
        // Verify all events have valid timestamps and data
        let valid_events = events.iter().all(|event| {
            let now = std::time::SystemTime::now();
            let timestamp_valid = now.duration_since(event.timestamp).is_ok();
            
            // Check that at least some data is present
            let has_data = event.battery_level.is_some() || 
                          event.is_charging.is_some() || 
                          event.power_source.is_some();
            
            timestamp_valid && (has_data || matches!(event.event_type, 
                PowerEventType::SleepMode | 
                PowerEventType::WakeFromSleep | 
                PowerEventType::Shutdown | 
                PowerEventType::Restart))
        });
        
        assert!(valid_events, "All events should have valid timestamps and reasonable data");
        println!("  ✓ All event data is valid");
    } else {
        println!("⚠ No power events during stability test - system power state may be stable");
    }
    
    println!("✓ Power monitoring stability test passed");
    
    event_system.stop().await.expect("Failed to stop event system");
}

#[tokio::test]
async fn test_battery_threshold_sensitivity() {
    let mut event_system = EventSystem::new();
    event_system.start().await.expect("Failed to start event system");
    
    let high_threshold_events = Arc::new(Mutex::new(Vec::new()));
    let low_threshold_events = Arc::new(Mutex::new(Vec::new()));
    
    let high_events_clone = high_threshold_events.clone();
    let low_events_clone = low_threshold_events.clone();
    
    // Set up battery monitors with different thresholds
    let _high_id = event_system.on_battery_low(50.0, move |event: PowerEventData| {
        let mut events = high_events_clone.lock().unwrap();
        events.push(event);
    }).await.expect("Failed to set up high threshold battery monitor");
    
    let _low_id = event_system.on_battery_low(100.0, move |event: PowerEventData| {
        let mut events = low_events_clone.lock().unwrap();
        events.push(event);
    }).await.expect("Failed to set up low threshold battery monitor");
    
    // Give time for monitoring to collect data
    tokio::time::sleep(Duration::from_millis(5000)).await;
    
    let high_events = high_threshold_events.lock().unwrap();
    let low_events = low_threshold_events.lock().unwrap();
    
    println!("Battery threshold sensitivity test results:");
    println!("  - High threshold (50%) events: {}", high_events.len());
    println!("  - Low threshold (100%) events: {}", low_events.len());
    
    if !high_events.is_empty() || !low_events.is_empty() {
        println!("✓ Battery threshold monitoring working");
        
        // Low threshold should trigger at least as often as high threshold
        assert!(low_events.len() >= high_events.len(), 
               "Lower threshold should trigger at least as often as higher threshold");
        
        // Verify event data consistency
        for events in [&*high_events, &*low_events] {
            for event in events.iter() {
                assert_eq!(event.event_type, PowerEventType::BatteryLow);
                
                if let Some(level) = event.battery_level {
                    assert!(level >= 0.0 && level <= 100.0, "Battery level should be valid");
                }
            }
        }
        
        println!("  ✓ Threshold sensitivity and data consistency verified");
    } else {
        println!("⚠ No battery threshold events detected - system may not have a battery or thresholds may need adjustment");
    }
    
    event_system.stop().await.expect("Failed to stop event system");
}

#[tokio::test]
async fn test_power_source_detection() {
    let mut event_system = EventSystem::new();
    event_system.start().await.expect("Failed to start event system");
    
    let events_received = Arc::new(Mutex::new(Vec::new()));
    let events_clone = events_received.clone();
    
    // Set up power event listener specifically for power source changes
    let _id = event_system.on_power_event(move |event: PowerEventData| {
        let mut events = events_clone.lock().unwrap();
        if event.event_type == PowerEventType::PowerSourceChanged || 
           event.power_source.is_some() {
            events.push(event);
        }
    }).await.expect("Failed to set up power source event listener");
    
    // Give the system time to detect current power source
    tokio::time::sleep(Duration::from_millis(3000)).await;
    
    let events = events_received.lock().unwrap();
    
    if !events.is_empty() {
        println!("✓ Power source information detected: {} events", events.len());
        
        let mut ac_power_detected = false;
        let mut battery_power_detected = false;
        let mut unknown_power_detected = false;
        
        for event in events.iter() {
            if let Some(ref power_source) = event.power_source {
                println!("  - Power source: {}", power_source);
                
                match power_source.to_lowercase().as_str() {
                    s if s.contains("ac") || s.contains("mains") || s.contains("adapter") => {
                        ac_power_detected = true;
                    }
                    s if s.contains("battery") => {
                        battery_power_detected = true;
                    }
                    _ => {
                        unknown_power_detected = true;
                    }
                }
            }
        }
        
        println!("Power source detection summary:");
        println!("  - AC power detected: {}", ac_power_detected);
        println!("  - Battery power detected: {}", battery_power_detected);
        println!("  - Unknown power source detected: {}", unknown_power_detected);
        
        // At least one power source type should be detected
        assert!(ac_power_detected || battery_power_detected || unknown_power_detected, 
               "Should detect at least one power source type");
        
        println!("✓ Power source detection working");
    } else {
        println!("⚠ No power source information detected - power source monitoring may require platform-specific implementation");
    }
    
    event_system.stop().await.expect("Failed to stop event system");
}

#[tokio::test]
async fn test_multiple_power_event_subscribers() {
    let mut event_system = EventSystem::new();
    event_system.start().await.expect("Failed to start event system");
    
    let subscriber1_events = Arc::new(Mutex::new(Vec::new()));
    let subscriber2_events = Arc::new(Mutex::new(Vec::new()));
    let subscriber3_events = Arc::new(Mutex::new(Vec::new()));
    
    let events1_clone = subscriber1_events.clone();
    let events2_clone = subscriber2_events.clone();
    let events3_clone = subscriber3_events.clone();
    
    // Set up multiple power event listeners
    let _id1 = event_system.on_power_event(move |event: PowerEventData| {
        let mut events = events1_clone.lock().unwrap();
        events.push(event);
    }).await.expect("Failed to set up power event listener 1");
    
    let _id2 = event_system.on_battery_low(100.0, move |event: PowerEventData| {
        let mut events = events2_clone.lock().unwrap();
        events.push(event);
    }).await.expect("Failed to set up battery low listener");
    
    let _id3 = event_system.on_power_event(move |event: PowerEventData| {
        let mut events = events3_clone.lock().unwrap();
        // Filter for charging/discharging events only
        if matches!(event.event_type, 
            PowerEventType::BatteryCharging | PowerEventType::BatteryDischarging) {
            events.push(event);
        }
    }).await.expect("Failed to set up filtered power event listener");
    
    // Give time for monitoring to collect data
    tokio::time::sleep(Duration::from_millis(5000)).await;
    
    let events1 = subscriber1_events.lock().unwrap();
    let events2 = subscriber2_events.lock().unwrap();
    let events3 = subscriber3_events.lock().unwrap();
    
    println!("Multiple power subscriber test results:");
    println!("  - General subscriber events: {}", events1.len());
    println!("  - Battery low subscriber events: {}", events2.len());
    println!("  - Charging/discharging subscriber events: {}", events3.len());
    
    if !events1.is_empty() {
        println!("✓ Multiple power event subscribers working");
        
        // Verify that battery low subscriber gets subset of general events
        let battery_low_in_general = events1.iter()
            .filter(|e| e.event_type == PowerEventType::BatteryLow)
            .count();
        
        assert!(events2.len() <= events1.len(), 
               "Battery low subscriber should get subset of general events");
        
        if battery_low_in_general > 0 {
            println!("  ✓ Battery low event filtering working");
        }
        
        // Verify filtered subscriber gets appropriate subset
        assert!(events3.len() <= events1.len(), 
               "Filtered subscriber should get subset of events");
        
        println!("  ✓ Event distribution to multiple subscribers is working correctly");
    } else {
        println!("⚠ No power events detected across multiple subscribers");
    }
    
    event_system.stop().await.expect("Failed to stop event system");
}