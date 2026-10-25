//! The plain data of one reading of the machine, as the sources return it.
//!
//! Counters are kept cumulative, as the machine gives them, because only the
//! history can tell whether two readings may be compared (see `history`).

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

/// Why a value is missing. Each tells the person something different, so the
/// reason is kept with the value and the window shows it, never a 0.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Missing {
    /// The hardware does not report it, such as a GPU with no fan (D18).
    NotReported,
    /// A rate needs two readings, and slm has only one yet.
    Waiting,
    /// A counter went backwards, or the interval, or the window it is
    /// averaged over, spans a stretch with no readings.
    NoRate,
    /// Reading it failed.
    Failed,
}

pub type Value<T> = Result<T, Missing>;

#[derive(Clone, Debug)]
pub struct Reading {
    /// Time since boot, on `CLOCK_BOOTTIME`, so that a suspend counts.
    pub time: Duration,
    /// The refresh interval in effect when the reading was taken.
    pub interval: Duration,
    pub cpu: Value<CpuStat>,
    /// The average of the cores' frequencies, in MHz.
    pub cpu_frequency: Value<f64>,
    /// In °C.
    pub cpu_temperature: Value<f64>,
    /// The sensor the temperature comes from, such as "k10temp Tctl" (D50).
    pub cpu_sensor: Option<&'static str>,
    /// What the CPU is (D50).
    pub cpu_info: Arc<CpuInfo>,
    pub memory: Value<Memory>,
    pub disks: Vec<DiskReading>,
    pub networks: Vec<NetworkReading>,
    pub gpu: GpuState,
}

/// What the CPU is, read once (D50).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CpuInfo {
    pub model: Option<String>,
    pub cores: Option<usize>,
    pub logical: Option<usize>,
}

/// Cumulative CPU times, in ticks, from `/proc/stat`.
#[derive(Clone, Debug, PartialEq)]
pub struct CpuStat {
    pub total: CpuTimes,
    pub cores: Vec<CpuTimes>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CpuTimes {
    /// Ticks neither idle nor waiting for input or output.
    pub busy: u64,
    pub all: u64,
}

/// In bytes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Memory {
    pub used: u64,
    pub total: u64,
    pub swap_used: u64,
    pub swap_total: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DiskReading {
    /// The device, such as `/dev/nvme0n1p1`, which names the entry (D32).
    pub device: String,
    pub mount_points: Vec<PathBuf>,
    /// In bytes.
    pub size: u64,
    /// In bytes, available to users, so that size less available counts the
    /// blocks reserved for root.
    pub available: u64,
    /// Cumulative bytes read and written.
    pub read: Value<u64>,
    pub written: Value<u64>,
    /// What the disk is (D50).
    pub info: Arc<DiskInfo>,
}

/// What a disk is, read once per device (D50).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DiskInfo {
    pub model: Option<String>,
    /// `Some(true)` for an SSD, `Some(false)` for an HDD.
    pub solid_state: Option<bool>,
    pub file_system: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct NetworkReading {
    pub interface: String,
    /// Cumulative bytes.
    pub received: u64,
    pub transmitted: u64,
    /// The link's speed, in Mb/s, and whether it is full duplex, read at
    /// each measurement, because a cable or a switch can change it. `None`
    /// when the link is down or its driver does not say.
    pub link: Option<(u32, bool)>,
    /// What the adapter is (D50).
    pub info: Arc<NetworkInfo>,
}

/// What a network adapter is, read once per adapter (D50).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NetworkInfo {
    /// The vendor and model, from udev's hardware database.
    pub model: Option<String>,
    pub driver: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum GpuState {
    /// The GPU source started, and these are its devices' readings.
    Running(Vec<GpuReading>),
    /// The GPU source could not start. `cards` lists the NVIDIA GPUs found on
    /// the PCI bus (D28), each of which gets a panel that says why (D20).
    NotStarted {
        failure: GpuFailure,
        cards: Vec<String>,
    },
}

/// Why the GPU source cannot read a GPU. Each has its own sentence, because
/// the next step differs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GpuFailure {
    /// The vendor's library could not be loaded: the driver is not installed.
    NoLibrary,
    /// The library is there, but the driver is not running.
    DriverNotLoaded,
    /// The library and the kernel module come from different driver versions.
    VersionMismatch,
    NoPermission,
    /// The GPU stopped responding while slm ran.
    Lost,
    Other,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GpuReading {
    /// A stable identifier, the NVML UUID, under which settings are stored (D31, D32).
    pub id: String,
    pub name: String,
    /// Set when the GPU stopped answering, in which case every value below is
    /// `Failed`.
    pub failure: Option<GpuFailure>,
    /// In percent.
    pub utilization: Value<f64>,
    /// In bytes.
    pub memory_used: Value<u64>,
    pub memory_total: Value<u64>,
    pub power: PowerReading,
    /// The limit the card enforces, in W.
    pub power_limit: Value<f64>,
    /// The range the card allows its limit to be set in, in W.
    pub power_limit_range: Value<(f64, f64)>,
    /// In °C.
    pub temperature: Value<f64>,
    /// Each fan's speed, in percent. `NotReported` when the GPU reports no fan.
    pub fans: Value<Vec<Value<f64>>>,
    /// The graphics clock and its maximum, in MHz.
    pub clock: Value<f64>,
    pub max_clock: Value<f64>,
    /// The PCIe link's generation and width now, which drop while the GPU is
    /// idle (D50).
    pub link: Value<(u32, u32)>,
    /// What the GPU is (D50).
    pub info: Arc<GpuInfo>,
}

/// What a GPU is, read once when the GPU source starts (D50).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GpuInfo {
    pub architecture: Option<String>,
    pub driver: Option<String>,
    /// The highest CUDA version the driver supports, major and minor.
    pub cuda: Option<(i32, i32)>,
    /// In °C, where the card slows down and where it shuts down.
    pub slowdown: Option<f64>,
    pub shutdown: Option<f64>,
    /// In W.
    pub default_limit: Option<f64>,
    /// The PCIe link's generation and width at most.
    pub max_link: Option<(u32, u32)>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PowerReading {
    /// The energy counter, in millijoules since the driver loaded. The
    /// history averages power from it.
    Energy(Value<u64>),
    /// One instantaneous reading, in W, on cards without the energy counter.
    Instant(Value<f64>),
}
