use crate::events::EventData;
use std::error::Error;
use async_trait::async_trait;

pub type EventCallback<T> = Box<dyn Fn(T) + Send + Sync>;
pub type AsyncEventCallback<T> = Box<dyn Fn(T) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>> + Send + Sync>;

#[async_trait]
pub trait EventHandler: Send + Sync {
    type EventType;
    type Config;
    
    async fn start(&mut self, config: Self::Config) -> crate::Result<()>;
    async fn stop(&mut self) -> crate::Result<()>;
    fn is_running(&self) -> bool;
    fn name(&self) -> &'static str;
}

#[async_trait]
pub trait EventSubscriber: Send + Sync {
    fn subscribe<F>(&mut self, callback: F) 
    where 
        F: Fn(EventData) + Send + Sync + 'static;
    
    fn subscribe_async<F, Fut>(&mut self, callback: F) 
    where 
        F: Fn(EventData) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = ()> + Send + 'static;
    
    fn unsubscribe(&mut self, id: usize) -> bool;
    fn clear_subscribers(&mut self);
}

pub trait EventFilter<T> {
    fn should_trigger(&self, event: &T) -> bool;
}

pub trait ThresholdConfig {
    fn set_threshold(&mut self, threshold: f32);
    fn get_threshold(&self) -> f32;
}

pub trait IntervalConfig {
    fn set_interval(&mut self, interval: std::time::Duration);
    fn get_interval(&self) -> std::time::Duration;
}

#[derive(Debug, Clone)]
pub struct EventHandlerConfig {
    pub enabled: bool,
    pub buffer_size: usize,
    pub poll_interval: std::time::Duration,
    pub debounce_duration: Option<std::time::Duration>,
}

impl Default for EventHandlerConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            buffer_size: 1000,
            poll_interval: std::time::Duration::from_millis(100),
            debounce_duration: Some(std::time::Duration::from_millis(50)),
        }
    }
}