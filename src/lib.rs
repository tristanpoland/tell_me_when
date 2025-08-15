pub mod events;
pub mod traits;
pub mod handlers;
pub mod event_system;

pub use event_system::EventSystem;
pub use events::*;
pub use traits::*;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use crossbeam_channel::{unbounded, Receiver, Sender};
use tokio::sync::RwLock;

pub type EventId = usize;
pub type HandlerId = String;

#[derive(thiserror::Error, Debug)]
pub enum TellMeWhenError {
    #[error("Handler not found: {0}")]
    HandlerNotFound(String),
    
    #[error("Handler already exists: {0}")]
    HandlerAlreadyExists(String),
    
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    
    #[error("System error: {0}")]
    System(String),
    
    #[error("Configuration error: {0}")]
    Config(String),
}

pub type Result<T> = std::result::Result<T, TellMeWhenError>;

#[derive(Debug, Clone)]
pub struct EventMetadata {
    pub id: EventId,
    pub handler_id: HandlerId,
    pub timestamp: std::time::SystemTime,
    pub source: String,
}

#[derive(Debug)]
pub struct EventMessage {
    pub metadata: EventMetadata,
    pub data: EventData,
}

pub struct EventBus {
    sender: Sender<EventMessage>,
    receiver: Receiver<EventMessage>,
    subscribers: Arc<RwLock<HashMap<EventId, Vec<Box<dyn Fn(EventMessage) + Send + Sync>>>>>,
    next_id: Arc<Mutex<EventId>>,
}

impl EventBus {
    pub fn new() -> Self {
        let (sender, receiver) = unbounded();
        Self {
            sender,
            receiver,
            subscribers: Arc::new(RwLock::new(HashMap::new())),
            next_id: Arc::new(Mutex::new(0)),
        }
    }

    pub fn sender(&self) -> Sender<EventMessage> {
        self.sender.clone()
    }

    pub async fn subscribe<F>(&self, callback: F) -> EventId
    where
        F: Fn(EventMessage) + Send + Sync + 'static,
    {
        let id = {
            let mut next_id = self.next_id.lock().unwrap();
            let id = *next_id;
            *next_id += 1;
            id
        };

        let mut subscribers = self.subscribers.write().await;
        subscribers.entry(id).or_insert_with(Vec::new).push(Box::new(callback));
        id
    }

    pub async fn unsubscribe(&self, id: EventId) -> bool {
        let mut subscribers = self.subscribers.write().await;
        subscribers.remove(&id).is_some()
    }

    pub async fn publish(&self, message: EventMessage) {
        if let Err(e) = self.sender.send(message) {
            log::error!("Failed to publish event: {}", e);
        }
    }

    pub async fn start_processing(&self) {
        let receiver = self.receiver.clone();
        let subscribers = self.subscribers.clone();
        
        tokio::spawn(async move {
            while let Ok(message) = receiver.recv() {
                let subscribers = subscribers.read().await;
                for callbacks in subscribers.values() {
                    for callback in callbacks {
                        callback(message.clone());
                    }
                }
            }
        });
    }
}

impl Clone for EventMessage {
    fn clone(&self) -> Self {
        Self {
            metadata: self.metadata.clone(),
            data: self.data.clone(),
        }
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}