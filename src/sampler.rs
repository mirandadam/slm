//! The sampling thread, which owns the sources. It reads them on a deadline
//! every refresh interval, whether or not the window is drawn, because a
//! minimized or hidden window may not be woken, and a slow read would
//! otherwise freeze the window.

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::gpu::{self, Gpus};
use crate::history::History;
use crate::reading::Reading;
use crate::system::{SystemReading, SystemSource};

/// Time since boot, which, unlike `std::time::Instant`, keeps counting while
/// the machine is suspended, so that a suspend shows at its real length (D17).
pub fn boottime() -> Duration {
    let mut time = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: `time` is a valid timespec for clock_gettime to write.
    let result = unsafe { libc::clock_gettime(libc::CLOCK_BOOTTIME, &mut time) };
    assert_eq!(
        result, 0,
        "CLOCK_BOOTTIME is available on every Linux since 2.6.39"
    );
    Duration::new(time.tv_sec as u64, time.tv_nsec as u32)
}

pub struct Sampler {
    read_system: Box<dyn FnMut() -> SystemReading + Send>,
    gpus: Gpus,
    clock: Box<dyn Fn() -> Duration + Send>,
    /// The refresh interval in milliseconds, which the window sets.
    interval: Arc<AtomicU64>,
    history: Arc<Mutex<History>>,
    due: Option<Duration>,
}

impl Sampler {
    /// Takes one reading, and returns the time the next one is due. A change
    /// of the refresh interval takes effect at the next reading (D3).
    pub fn sample(&mut self) -> Duration {
        let interval = Duration::from_millis(self.interval.load(Ordering::Relaxed));
        let time = (self.clock)();
        let system = (self.read_system)();
        let gpu = self.gpus.read(time);
        let reading = Reading {
            time,
            interval,
            cpu: system.cpu,
            cpu_frequency: system.cpu_frequency,
            cpu_temperature: system.cpu_temperature,
            memory: system.memory,
            disks: system.disks,
            networks: system.networks,
            gpu,
        };
        self.history.lock().unwrap().push(reading);
        // Deadlines follow one another, so that readings do not drift. One
        // missed, as after a suspend, starts the deadlines again from now.
        let now = (self.clock)();
        let due = self.due.unwrap_or(time) + interval;
        let due = if due <= now { now + interval } else { due };
        self.due = Some(due);
        due
    }
}

/// Starts the sampling thread. The thread starts the GPU library itself, so
/// that the window opens without waiting for it.
pub fn spawn(
    interval: Arc<AtomicU64>,
    history: Arc<Mutex<History>>,
    on_reading: impl Fn() + Send + 'static,
) {
    std::thread::Builder::new()
        .name("sampler".into())
        .spawn(move || {
            let mut system = SystemSource::new();
            let mut sampler = Sampler {
                read_system: Box::new(move || system.read()),
                gpus: Gpus::new(
                    Box::new(gpu::nvml::start),
                    Path::new("/sys/bus/pci/devices"),
                ),
                clock: Box::new(boottime),
                interval,
                history,
                due: None,
            };
            loop {
                let due = sampler.sample();
                on_reading();
                std::thread::sleep(due.saturating_sub(boottime()));
            }
        })
        .expect("the sampling thread starts");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reading::{GpuFailure, Missing};

    #[test]
    fn d3_a_change_of_the_refresh_interval_takes_effect_at_the_next_reading() {
        let now = Arc::new(Mutex::new(Duration::ZERO));
        let interval = Arc::new(AtomicU64::new(500));
        let history = Arc::new(Mutex::new(History::default()));
        let clock = now.clone();
        let mut sampler = Sampler {
            read_system: Box::new(|| SystemReading {
                cpu: Err(Missing::Failed),
                cpu_frequency: Err(Missing::Failed),
                cpu_temperature: Err(Missing::Failed),
                memory: Err(Missing::Failed),
                disks: Vec::new(),
                networks: Vec::new(),
            }),
            gpus: Gpus::new(
                Box::new(|| Err(GpuFailure::NoLibrary)),
                Path::new("/nonexistent"),
            ),
            clock: Box::new(move || *clock.lock().unwrap()),
            interval: interval.clone(),
            history: history.clone(),
            due: None,
        };
        let ms = Duration::from_millis;
        let mut dues = Vec::new();
        let mut step = |at: u64, sampler: &mut Sampler| {
            *now.lock().unwrap() = ms(at);
            dues.push(sampler.sample());
        };
        step(0, &mut sampler);
        step(500, &mut sampler);
        interval.store(200, Ordering::Relaxed);
        step(1000, &mut sampler);
        step(1200, &mut sampler);
        // Late by 2 s: the deadlines start again from now.
        step(3400, &mut sampler);
        assert_eq!(dues, [ms(500), ms(1000), ms(1200), ms(1400), ms(3600)]);
        let intervals: Vec<_> = (history.lock().unwrap().entries().iter())
            .map(|e| e.reading.interval)
            .collect();
        assert_eq!(intervals, [ms(500), ms(500), ms(200), ms(200), ms(200)]);
    }
}
