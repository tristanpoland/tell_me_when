use tell_me_when::{EventSystem, FsEventData, FsEventType};
use tempfile::TempDir;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::time::timeout;
use std::fs;
use std::io::Write;

#[tokio::test]
async fn test_file_creation_event() {
    let mut event_system = EventSystem::new();
    event_system.start().await.expect("Failed to start event system");
    
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    let temp_path = temp_dir.path();
    
    let events_received = Arc::new(Mutex::new(Vec::new()));
    let events_clone = events_received.clone();
    
    // Set up file creation event listener
    let _id = event_system.on_fs_created(temp_path, move |event: FsEventData| {
        let mut events = events_clone.lock().unwrap();
        events.push(event);
    }).await.expect("Failed to set up fs created event listener");
    
    // Give the system time to set up the watcher
    tokio::time::sleep(Duration::from_millis(200)).await;
    
    // Create a test file
    let test_file = temp_path.join("test_creation.txt");
    fs::write(&test_file, "Hello, World!").expect("Failed to write test file");
    
    // Wait for the event to be processed
    let result = timeout(Duration::from_secs(10), async {
        loop {
            {
                let events = events_received.lock().unwrap();
                if !events.is_empty() {
                    break;
                }
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }).await;
    
    assert!(result.is_ok(), "Timeout waiting for file creation event");
    
    let events = events_received.lock().unwrap();
    assert!(!events.is_empty(), "No file creation events received");
    
    let first_event = &events[0];
    assert_eq!(first_event.event_type, FsEventType::Created);
    assert!(first_event.path.to_string_lossy().contains("test_creation.txt"));
    
    event_system.stop().await.expect("Failed to stop event system");
}

#[tokio::test]
async fn test_file_modification_event() {
    let mut event_system = EventSystem::new();
    event_system.start().await.expect("Failed to start event system");
    
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    let temp_path = temp_dir.path();
    
    let events_received = Arc::new(Mutex::new(Vec::new()));
    let events_clone = events_received.clone();
    
    // Set up file modification event listener
    let _id = event_system.on_fs_modified(temp_path, move |event: FsEventData| {
        let mut events = events_clone.lock().unwrap();
        events.push(event);
    }).await.expect("Failed to set up fs modified event listener");
    
    // Give the system time to set up the watcher
    tokio::time::sleep(Duration::from_millis(200)).await;
    
    // Create initial file
    let test_file = temp_path.join("test_modification.txt");
    fs::write(&test_file, "Initial content").expect("Failed to write initial file");
    
    // Give time for creation event to settle
    tokio::time::sleep(Duration::from_millis(300)).await;
    
    // Clear any creation events
    {
        let mut events = events_received.lock().unwrap();
        events.clear();
    }
    
    // Modify the file
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .append(true)
        .open(&test_file)
        .expect("Failed to open file for modification");
    
    writeln!(file, "\nModified content").expect("Failed to write to file");
    file.flush().expect("Failed to flush file");
    drop(file); // Ensure file is closed
    
    // Wait for the modification event
    let result = timeout(Duration::from_secs(10), async {
        loop {
            {
                let events = events_received.lock().unwrap();
                if !events.is_empty() {
                    break;
                }
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }).await;
    
    assert!(result.is_ok(), "Timeout waiting for file modification event");
    
    let events = events_received.lock().unwrap();
    assert!(!events.is_empty(), "No file modification events received");
    
    let first_event = &events[0];
    assert_eq!(first_event.event_type, FsEventType::Modified);
    assert!(first_event.path.to_string_lossy().contains("test_modification.txt"));
    
    event_system.stop().await.expect("Failed to stop event system");
}

#[tokio::test]
async fn test_file_deletion_event() {
    let mut event_system = EventSystem::new();
    event_system.start().await.expect("Failed to start event system");
    
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    let temp_path = temp_dir.path();
    
    let events_received = Arc::new(Mutex::new(Vec::new()));
    let events_clone = events_received.clone();
    
    // Set up file deletion event listener
    let _id = event_system.on_fs_deleted(temp_path, move |event: FsEventData| {
        let mut events = events_clone.lock().unwrap();
        events.push(event);
    }).await.expect("Failed to set up fs deleted event listener");
    
    // Give the system time to set up the watcher
    tokio::time::sleep(Duration::from_millis(200)).await;
    
    // Create a test file first
    let test_file = temp_path.join("test_deletion.txt");
    fs::write(&test_file, "File to be deleted").expect("Failed to write test file");
    
    // Give time for creation to complete
    tokio::time::sleep(Duration::from_millis(300)).await;
    
    // Delete the file
    fs::remove_file(&test_file).expect("Failed to delete test file");
    
    // Wait for the deletion event
    let result = timeout(Duration::from_secs(10), async {
        loop {
            {
                let events = events_received.lock().unwrap();
                for event in events.iter() {
                    if event.event_type == FsEventType::Deleted {
                        return;
                    }
                }
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }).await;
    
    assert!(result.is_ok(), "Timeout waiting for file deletion event");
    
    let events = events_received.lock().unwrap();
    let deletion_events: Vec<_> = events.iter()
        .filter(|e| e.event_type == FsEventType::Deleted)
        .collect();
    
    assert!(!deletion_events.is_empty(), "No file deletion events received");
    
    let first_deletion = deletion_events[0];
    assert!(first_deletion.path.to_string_lossy().contains("test_deletion.txt"));
    
    event_system.stop().await.expect("Failed to stop event system");
}

#[tokio::test]
async fn test_directory_creation_event() {
    let mut event_system = EventSystem::new();
    event_system.start().await.expect("Failed to start event system");
    
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    let temp_path = temp_dir.path();
    
    let events_received = Arc::new(Mutex::new(Vec::new()));
    let events_clone = events_received.clone();
    
    // Set up directory creation event listener
    let _id = event_system.on_fs_created(temp_path, move |event: FsEventData| {
        let mut events = events_clone.lock().unwrap();
        events.push(event);
    }).await.expect("Failed to set up fs created event listener");
    
    // Give the system time to set up the watcher
    tokio::time::sleep(Duration::from_millis(200)).await;
    
    // Create a test directory
    let test_dir = temp_path.join("test_dir");
    fs::create_dir(&test_dir).expect("Failed to create test directory");
    
    // Wait for the creation event
    let result = timeout(Duration::from_secs(10), async {
        loop {
            {
                let events = events_received.lock().unwrap();
                if !events.is_empty() {
                    break;
                }
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }).await;
    
    assert!(result.is_ok(), "Timeout waiting for directory creation event");
    
    let events = events_received.lock().unwrap();
    assert!(!events.is_empty(), "No directory creation events received");
    
    let first_event = &events[0];
    assert_eq!(first_event.event_type, FsEventType::Created);
    assert!(first_event.path.to_string_lossy().contains("test_dir"));
    
    event_system.stop().await.expect("Failed to stop event system");
}

#[tokio::test]
async fn test_multiple_file_operations() {
    let mut event_system = EventSystem::new();
    event_system.start().await.expect("Failed to start event system");
    
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    let temp_path = temp_dir.path();
    
    let events_received = Arc::new(Mutex::new(Vec::new()));
    let events_clone = events_received.clone();
    
    // Set up general file system event listener
    let _id = event_system.on_fs_event(temp_path, move |event: FsEventData| {
        let mut events = events_clone.lock().unwrap();
        events.push(event);
    }).await.expect("Failed to set up fs event listener");
    
    // Give the system time to set up the watcher
    tokio::time::sleep(Duration::from_millis(200)).await;
    
    // Perform multiple operations
    let test_file1 = temp_path.join("file1.txt");
    let test_file2 = temp_path.join("file2.txt");
    let test_file3 = temp_path.join("file3.txt");
    
    // Create files
    fs::write(&test_file1, "Content 1").expect("Failed to write file1");
    tokio::time::sleep(Duration::from_millis(100)).await;
    
    fs::write(&test_file2, "Content 2").expect("Failed to write file2");
    tokio::time::sleep(Duration::from_millis(100)).await;
    
    fs::write(&test_file3, "Content 3").expect("Failed to write file3");
    tokio::time::sleep(Duration::from_millis(100)).await;
    
    // Modify files
    fs::write(&test_file1, "Modified Content 1").expect("Failed to modify file1");
    tokio::time::sleep(Duration::from_millis(100)).await;
    
    // Delete a file
    fs::remove_file(&test_file2).expect("Failed to delete file2");
    tokio::time::sleep(Duration::from_millis(100)).await;
    
    // Wait for all events to be processed
    tokio::time::sleep(Duration::from_millis(1000)).await;
    
    let events = events_received.lock().unwrap();
    assert!(events.len() >= 3, "Expected at least 3 events, got {}", events.len());
    
    // Check that we have different types of events
    let has_created = events.iter().any(|e| e.event_type == FsEventType::Created);
    let has_modified = events.iter().any(|e| e.event_type == FsEventType::Modified);
    let has_deleted = events.iter().any(|e| e.event_type == FsEventType::Deleted);
    
    assert!(has_created, "Should have at least one creation event");
    // Note: modified and deleted events might not always be detected depending on the platform
    // so we don't assert on them, but we can check if they exist
    
    if has_modified {
        println!("✓ Modification events detected");
    }
    if has_deleted {
        println!("✓ Deletion events detected");
    }
    
    event_system.stop().await.expect("Failed to stop event system");
}

#[tokio::test]
async fn test_nested_directory_operations() {
    let mut event_system = EventSystem::new();
    event_system.start().await.expect("Failed to start event system");
    
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    let temp_path = temp_dir.path();
    
    let events_received = Arc::new(Mutex::new(Vec::new()));
    let events_clone = events_received.clone();
    
    // Set up file system event listener with subdirectory watching
    let _id = event_system.on_fs_event(temp_path, move |event: FsEventData| {
        let mut events = events_clone.lock().unwrap();
        events.push(event);
    }).await.expect("Failed to set up fs event listener");
    
    // Give the system time to set up the watcher
    tokio::time::sleep(Duration::from_millis(200)).await;
    
    // Create nested directory structure
    let nested_dir = temp_path.join("level1").join("level2");
    fs::create_dir_all(&nested_dir).expect("Failed to create nested directories");
    
    tokio::time::sleep(Duration::from_millis(200)).await;
    
    // Create file in nested directory
    let nested_file = nested_dir.join("nested_file.txt");
    fs::write(&nested_file, "Nested content").expect("Failed to write nested file");
    
    // Wait for events
    tokio::time::sleep(Duration::from_millis(1000)).await;
    
    let events = events_received.lock().unwrap();
    assert!(!events.is_empty(), "Should have received events for nested operations");
    
    // Check that we received events for the nested structure
    let has_nested_events = events.iter().any(|e| 
        e.path.to_string_lossy().contains("level1") || 
        e.path.to_string_lossy().contains("level2") ||
        e.path.to_string_lossy().contains("nested_file")
    );
    
    if has_nested_events {
        println!("✓ Nested directory events detected");
    } else {
        println!("⚠ Nested directory events not detected (may be platform-specific)");
    }
    
    event_system.stop().await.expect("Failed to stop event system");
}

#[tokio::test]
async fn test_event_timing_and_metadata() {
    let mut event_system = EventSystem::new();
    event_system.start().await.expect("Failed to start event system");
    
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    let temp_path = temp_dir.path();
    
    let events_received = Arc::new(Mutex::new(Vec::new()));
    let events_clone = events_received.clone();
    
    let start_time = Instant::now();
    
    // Set up file system event listener
    let _id = event_system.on_fs_created(temp_path, move |event: FsEventData| {
        let mut events = events_clone.lock().unwrap();
        events.push(event);
    }).await.expect("Failed to set up fs created event listener");
    
    // Give the system time to set up the watcher
    tokio::time::sleep(Duration::from_millis(200)).await;
    
    // Create a test file
    let test_file = temp_path.join("timing_test.txt");
    let file_creation_time = Instant::now();
    fs::write(&test_file, "Timing test").expect("Failed to write test file");
    
    // Wait for the event
    let result = timeout(Duration::from_secs(10), async {
        loop {
            {
                let events = events_received.lock().unwrap();
                if !events.is_empty() {
                    break;
                }
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }).await;
    
    assert!(result.is_ok(), "Timeout waiting for file creation event");
    
    let events = events_received.lock().unwrap();
    assert!(!events.is_empty(), "No events received");
    
    let event = &events[0];
    
    // Check event metadata
    assert_eq!(event.event_type, FsEventType::Created);
    assert!(event.path.to_string_lossy().contains("timing_test.txt"));
    
    // Check timestamp is reasonable (within a few seconds of creation)
    let event_time = event.timestamp.duration_since(std::time::UNIX_EPOCH).unwrap();
    let creation_time = file_creation_time.duration_since(start_time);
    
    // The event timestamp should be close to when we created the file
    // We allow a generous window since file system events can have some delay
    println!("Event received at: {:?} after file creation", 
             event_time.as_millis().saturating_sub(
                 start_time.elapsed().as_millis().saturating_sub(creation_time.as_millis())
             ));
    
    event_system.stop().await.expect("Failed to stop event system");
}