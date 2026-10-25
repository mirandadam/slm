//! The GPUs, through a vendor-neutral interface (D4), so that another
//! vendor adds a source beside `nvml` without changing the rest.

pub mod nvml;

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::reading::{GpuFailure, GpuReading, GpuState};

/// One GPU that slm can read.
pub trait GpuSource: Send {
    fn read(&mut self) -> GpuReading;
}

/// Starts a vendor's library and lists the GPUs it can read.
pub type Start = Box<dyn FnMut() -> Result<Vec<Box<dyn GpuSource>>, GpuFailure> + Send>;

/// How long slm waits before it starts the GPU library again, while it
/// cannot start or after a GPU stops responding, so that a driver fixed while
/// slm runs shows without a restart.
const RETRY: Duration = Duration::from_secs(10);

pub struct Gpus {
    start: Start,
    sources: Result<Vec<Box<dyn GpuSource>>, GpuFailure>,
    pci_devices: PathBuf,
    /// The NVIDIA GPUs on the PCI bus, found only when the library cannot start.
    cards: Vec<String>,
    retry_at: Option<Duration>,
}

impl Gpus {
    pub fn new(start: Start, pci_devices: &Path) -> Self {
        let mut gpus = Self {
            start,
            sources: Err(GpuFailure::Other),
            pci_devices: pci_devices.to_path_buf(),
            cards: Vec::new(),
            retry_at: None,
        };
        gpus.start();
        gpus
    }

    fn start(&mut self) {
        self.sources = (self.start)();
        self.cards = match self.sources {
            Ok(_) => Vec::new(),
            Err(_) => nvidia_cards(&self.pci_devices),
        };
    }

    /// Reads every GPU. `now` is the reading's time, which paces the retries.
    pub fn read(&mut self, now: Duration) -> GpuState {
        if self.sources.is_err() {
            let retry_at = *self.retry_at.get_or_insert(now + RETRY);
            if now >= retry_at {
                self.start();
                self.retry_at = self.sources.is_err().then_some(now + RETRY);
            }
        }
        let sources = match &mut self.sources {
            Ok(sources) => sources,
            Err(failure) => {
                return GpuState::NotStarted {
                    failure: *failure,
                    cards: self.cards.clone(),
                };
            }
        };
        let readings: Vec<GpuReading> = sources.iter_mut().map(|s| s.read()).collect();
        if !readings.iter().any(|r| r.failure == Some(GpuFailure::Lost)) {
            self.retry_at = None;
            return GpuState::Running(readings);
        }
        let retry_at = *self.retry_at.get_or_insert(now + RETRY);
        if now < retry_at {
            return GpuState::Running(readings);
        }
        match (self.start)() {
            Ok(mut sources) => {
                let readings: Vec<GpuReading> = sources.iter_mut().map(|s| s.read()).collect();
                let lost = readings.iter().any(|r| r.failure == Some(GpuFailure::Lost));
                self.retry_at = lost.then_some(now + RETRY);
                self.sources = Ok(sources);
                GpuState::Running(readings)
            }
            Err(_) => {
                self.retry_at = Some(now + RETRY);
                GpuState::Running(readings)
            }
        }
    }
}

/// The NVIDIA display controllers on the PCI bus, by address (D28). The
/// bus lists a card whether or not a driver is bound to it, unlike
/// `/sys/class/drm`, so it finds the card whose NVIDIA driver failed to load.
pub fn nvidia_cards(pci_devices: &Path) -> Vec<String> {
    let read =
        |dir: &PathBuf, file: &str| std::fs::read_to_string(dir.join(file)).unwrap_or_default();
    let Ok(entries) = std::fs::read_dir(pci_devices) else {
        return Vec::new();
    };
    let mut cards: Vec<String> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|dir| {
            read(dir, "class").starts_with("0x03") && read(dir, "vendor").trim() == "0x10de"
        })
        .filter_map(|dir| Some(dir.file_name()?.to_string_lossy().into_owned()))
        .collect();
    cards.sort();
    cards
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::tests::gpu;
    use crate::reading::{Missing, PowerReading};
    use std::sync::{Arc, Mutex};

    struct Fake {
        reading: Arc<Mutex<GpuReading>>,
    }

    impl GpuSource for Fake {
        fn read(&mut self) -> GpuReading {
            self.reading.lock().unwrap().clone()
        }
    }

    fn pci_tree(name: &str, devices: &[(&str, &str, &str)]) -> PathBuf {
        let root = std::env::temp_dir().join(format!("slm-pci-{name}-{}", std::process::id()));
        for (address, class, vendor) in devices {
            let dir = root.join(address);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("class"), format!("{class}\n")).unwrap();
            std::fs::write(dir.join("vendor"), format!("{vendor}\n")).unwrap();
        }
        root
    }

    #[test]
    fn d28_nvidia_display_controllers_are_found_on_the_pci_bus() {
        let root = pci_tree(
            "d28",
            &[
                ("0000:07:00.0", "0x030000", "0x10de"),
                ("0000:07:00.1", "0x040300", "0x10de"),
                ("0000:0a:00.0", "0x030200", "0x10de"),
                ("0000:0c:00.0", "0x030000", "0x1002"),
            ],
        );
        let failing = Gpus::new(Box::new(|| Err(GpuFailure::DriverNotLoaded)), &root);
        let started = Gpus::new(
            Box::new(|| {
                let reading = gpu("GPU-1", PowerReading::Energy(Ok(0)), Ok(185.0));
                Ok(vec![Box::new(Fake {
                    reading: Arc::new(Mutex::new(reading)),
                }) as Box<dyn GpuSource>])
            }),
            &root,
        );
        std::fs::remove_dir_all(&root).unwrap();

        let mut failing = failing;
        assert_eq!(
            failing.read(Duration::ZERO),
            GpuState::NotStarted {
                failure: GpuFailure::DriverNotLoaded,
                cards: vec!["0000:07:00.0".into(), "0000:0a:00.0".into()],
            }
        );
        let mut started = started;
        let GpuState::Running(readings) = started.read(Duration::ZERO) else {
            panic!("not running");
        };
        assert_eq!(readings.len(), 1);
        assert_eq!(readings[0].id, "GPU-1");
    }

    #[test]
    fn while_the_library_cannot_start_it_is_started_again_every_10_s() {
        let starts = Arc::new(Mutex::new(0));
        let count = starts.clone();
        let mut gpus = Gpus::new(
            Box::new(move || {
                let mut count = count.lock().unwrap();
                *count += 1;
                if *count < 3 {
                    return Err(GpuFailure::DriverNotLoaded);
                }
                Ok(vec![Box::new(Fake {
                    reading: Arc::new(Mutex::new(gpu(
                        "GPU-1",
                        PowerReading::Energy(Ok(0)),
                        Ok(185.0),
                    ))),
                }) as Box<dyn GpuSource>])
            }),
            Path::new("/nonexistent"),
        );
        let secs = Duration::from_secs;
        for t in [0, 5, 9] {
            assert!(matches!(gpus.read(secs(t)), GpuState::NotStarted { .. }));
        }
        assert!(matches!(gpus.read(secs(10)), GpuState::NotStarted { .. }));
        assert_eq!(*starts.lock().unwrap(), 2);
        assert!(matches!(gpus.read(secs(15)), GpuState::NotStarted { .. }));
        assert!(matches!(gpus.read(secs(20)), GpuState::Running(_)));
        gpus.read(secs(40));
        assert_eq!(*starts.lock().unwrap(), 3);
    }

    #[test]
    fn after_a_gpu_stops_responding_the_library_is_started_again_every_10_s() {
        let lost = GpuReading {
            failure: Some(GpuFailure::Lost),
            utilization: Err(Missing::Failed),
            ..gpu(
                "GPU-1",
                PowerReading::Energy(Err(Missing::Failed)),
                Err(Missing::Failed),
            )
        };
        let shared = Arc::new(Mutex::new(lost));
        let starts = Arc::new(Mutex::new(0));
        let (reading, count) = (shared.clone(), starts.clone());
        let mut gpus = Gpus::new(
            Box::new(move || {
                *count.lock().unwrap() += 1;
                Ok(vec![Box::new(Fake {
                    reading: reading.clone(),
                }) as Box<dyn GpuSource>])
            }),
            Path::new("/nonexistent"),
        );
        let secs = Duration::from_secs;
        for t in [0, 5, 9] {
            gpus.read(secs(t));
        }
        assert_eq!(*starts.lock().unwrap(), 1);
        gpus.read(secs(10));
        assert_eq!(*starts.lock().unwrap(), 2);
        gpus.read(secs(15));
        assert_eq!(*starts.lock().unwrap(), 2);
        gpus.read(secs(20));
        assert_eq!(*starts.lock().unwrap(), 3);
        // Once it responds again, the library is left running.
        *shared.lock().unwrap() = gpu("GPU-1", PowerReading::Energy(Ok(0)), Ok(185.0));
        for t in [25, 30, 40] {
            gpus.read(secs(t));
        }
        assert_eq!(*starts.lock().unwrap(), 3);
    }
}
