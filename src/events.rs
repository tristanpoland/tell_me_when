use std::path::PathBuf;
use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum FsEventType {
    Created,
    Modified,
    Deleted,
    Renamed { old_path: PathBuf, new_path: PathBuf },
    Moved { from: PathBuf, to: PathBuf },
    AttributeChanged,
    PermissionChanged,
}

#[derive(Debug, Clone)]
pub struct FsEventData {
    pub event_type: FsEventType,
    pub path: PathBuf,
    pub timestamp: std::time::SystemTime,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ProcessEventType {
    Started,
    Terminated,
    CpuUsageHigh,
    MemoryUsageHigh,
    StatusChanged,
}

#[derive(Debug, Clone)]
pub struct ProcessEventData {
    pub event_type: ProcessEventType,
    pub pid: u32,
    pub name: String,
    pub cpu_usage: Option<f32>,
    pub memory_usage: Option<u64>,
    pub timestamp: std::time::SystemTime,
}

#[derive(Debug, Clone, PartialEq)]
pub enum NetworkEventType {
    InterfaceUp,
    InterfaceDown,
    ConnectionEstablished,
    ConnectionLost,
    TrafficThresholdReached,
}

#[derive(Debug, Clone)]
pub struct NetworkEventData {
    pub event_type: NetworkEventType,
    pub interface_name: Option<String>,
    pub local_addr: Option<String>,
    pub remote_addr: Option<String>,
    pub bytes_sent: Option<u64>,
    pub bytes_received: Option<u64>,
    pub timestamp: std::time::SystemTime,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SystemEventType {
    CpuUsageHigh,
    MemoryUsageHigh,
    DiskSpaceLow,
    TemperatureHigh,
    LoadAverageHigh,
}

#[derive(Debug, Clone)]
pub struct SystemEventData {
    pub event_type: SystemEventType,
    pub cpu_usage: Option<f32>,
    pub memory_usage: Option<f32>,
    pub disk_usage: Option<f32>,
    pub temperature: Option<f32>,
    pub load_average: Option<f32>,
    pub timestamp: std::time::SystemTime,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PowerEventType {
    BatteryLow,
    BatteryCharging,
    BatteryDischarging,
    PowerSourceChanged,
    SleepMode,
    WakeFromSleep,
    Shutdown,
    Restart,
}

#[derive(Debug, Clone)]
pub struct PowerEventData {
    pub event_type: PowerEventType,
    pub battery_level: Option<f32>,
    pub is_charging: Option<bool>,
    pub power_source: Option<String>,
    pub timestamp: std::time::SystemTime,
}

#[derive(Debug, Clone)]
pub enum EventData {
    FileSystem(FsEventData),
    Process(ProcessEventData),
    Network(NetworkEventData),
    System(SystemEventData),
    Power(PowerEventData),
}

impl fmt::Display for FsEventType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FsEventType::Created => write!(f, "Created"),
            FsEventType::Modified => write!(f, "Modified"),
            FsEventType::Deleted => write!(f, "Deleted"),
            FsEventType::Renamed { old_path, new_path } => {
                write!(f, "Renamed from {:?} to {:?}", old_path, new_path)
            }
            FsEventType::Moved { from, to } => {
                write!(f, "Moved from {:?} to {:?}", from, to)
            }
            FsEventType::AttributeChanged => write!(f, "AttributeChanged"),
            FsEventType::PermissionChanged => write!(f, "PermissionChanged"),
        }
    }
}

impl fmt::Display for ProcessEventType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProcessEventType::Started => write!(f, "Started"),
            ProcessEventType::Terminated => write!(f, "Terminated"),
            ProcessEventType::CpuUsageHigh => write!(f, "CpuUsageHigh"),
            ProcessEventType::MemoryUsageHigh => write!(f, "MemoryUsageHigh"),
            ProcessEventType::StatusChanged => write!(f, "StatusChanged"),
        }
    }
}