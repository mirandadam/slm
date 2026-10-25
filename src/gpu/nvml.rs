//! NVIDIA GPUs, through NVML. `nvml-wrapper` loads `libnvidia-ml.so.1` when
//! slm starts, so slm also starts on a machine without the NVIDIA driver.

use std::sync::Arc;

use nvml_wrapper::Nvml;
use nvml_wrapper::enum_wrappers::device::{Clock, TemperatureSensor};
use nvml_wrapper::error::NvmlError;

use super::GpuSource;
use crate::reading::{GpuFailure, GpuReading, Missing, PowerReading, Value};

/// Starts NVML and lists its GPUs.
pub fn start() -> Result<Vec<Box<dyn GpuSource>>, GpuFailure> {
    let nvml = Arc::new(Nvml::init().map_err(|e| start_failure(&e))?);
    let count = nvml.device_count().map_err(|e| start_failure(&e))?;
    (0..count)
        .map(|index| {
            let device = nvml.device_by_index(index).map_err(|e| start_failure(&e))?;
            // A card whose UUID cannot be read fails the start for every card,
            // because thresholds and hidden lines are kept under the UUID (D31,
            // D32), and a card keyed by anything else would lose them.
            let id = device.uuid().map_err(|e| start_failure(&e))?;
            Ok(Box::new(NvmlGpu {
                nvml: nvml.clone(),
                name: device
                    .name()
                    .ok()
                    .filter(|n| !n.is_empty())
                    .unwrap_or_else(|| id.clone()),
                id,
            }) as Box<dyn GpuSource>)
        })
        .collect()
}

/// A GPU cannot stop responding before slm has seen it respond, so at start
/// the errors that mean so give the general failure.
fn start_failure(error: &NvmlError) -> GpuFailure {
    match failure(error) {
        GpuFailure::Lost => GpuFailure::Other,
        failure => failure,
    }
}

fn failure(error: &NvmlError) -> GpuFailure {
    match error {
        NvmlError::LibloadingError(_) | NvmlError::LibraryNotFound => GpuFailure::NoLibrary,
        NvmlError::DriverNotLoaded => GpuFailure::DriverNotLoaded,
        NvmlError::LibRmVersionMismatch => GpuFailure::VersionMismatch,
        NvmlError::NoPermission => GpuFailure::NoPermission,
        NvmlError::GpuLost | NvmlError::ResetRequired | NvmlError::Unknown => GpuFailure::Lost,
        _ => GpuFailure::Other,
    }
}

fn value<T>(result: Result<T, NvmlError>) -> Value<T> {
    result.map_err(|e| match e {
        NvmlError::NotSupported => Missing::NotReported,
        _ => Missing::Failed,
    })
}

/// The energy counter where the card has one (Volta and newer), and
/// otherwise one instantaneous reading.
fn power(
    energy: Result<u64, NvmlError>,
    instant: impl FnOnce() -> Result<u32, NvmlError>,
) -> PowerReading {
    match energy {
        Err(NvmlError::NotSupported) => {
            PowerReading::Instant(value(instant()).map(|mw| mw as f64 / 1000.0))
        }
        energy => PowerReading::Energy(value(energy)),
    }
}

struct NvmlGpu {
    nvml: Arc<Nvml>,
    id: String,
    name: String,
}

impl NvmlGpu {
    fn lost(&self) -> GpuReading {
        GpuReading {
            id: self.id.clone(),
            name: self.name.clone(),
            failure: Some(GpuFailure::Lost),
            utilization: Err(Missing::Failed),
            memory_used: Err(Missing::Failed),
            memory_total: Err(Missing::Failed),
            power: PowerReading::Energy(Err(Missing::Failed)),
            power_limit: Err(Missing::Failed),
            power_limit_range: Err(Missing::Failed),
            temperature: Err(Missing::Failed),
            fans: Err(Missing::Failed),
            clock: Err(Missing::Failed),
            max_clock: Err(Missing::Failed),
        }
    }
}

impl GpuSource for NvmlGpu {
    fn read(&mut self) -> GpuReading {
        let Ok(device) = self.nvml.device_by_uuid(self.id.as_str()) else {
            return self.lost();
        };
        let utilization = match device.utilization_rates() {
            Err(e) if failure(&e) == GpuFailure::Lost => return self.lost(),
            result => value(result).map(|u| u.gpu as f64),
        };
        let memory = value(device.memory_info());
        let fans = match value(device.num_fans()) {
            Ok(0) => Err(Missing::NotReported),
            count => count.map(|n| {
                (0..n)
                    .map(|i| value(device.fan_speed(i)).map(f64::from))
                    .collect()
            }),
        };
        let watts = |mw: u32| mw as f64 / 1000.0;
        GpuReading {
            id: self.id.clone(),
            name: self.name.clone(),
            failure: None,
            utilization,
            memory_used: memory.as_ref().map(|m| m.used).map_err(|e| *e),
            memory_total: memory.map(|m| m.total),
            power: power(device.total_energy_consumption(), || device.power_usage()),
            power_limit: value(device.enforced_power_limit()).map(watts),
            power_limit_range: value(device.power_management_limit_constraints())
                .map(|c| (watts(c.min_limit), watts(c.max_limit))),
            temperature: value(device.temperature(TemperatureSensor::Gpu)).map(f64::from),
            fans,
            clock: value(device.clock_info(Clock::Graphics)).map(f64::from),
            max_clock: value(device.max_clock_info(Clock::Graphics)).map(f64::from),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn d1_on_a_card_without_the_energy_counter_the_reading_is_instantaneous() {
        assert_eq!(
            power(Err(NvmlError::NotSupported), || Ok(157_250)),
            PowerReading::Instant(Ok(157.25))
        );
        assert_eq!(
            power(Ok(5_000), || panic!("not read")),
            PowerReading::Energy(Ok(5_000))
        );
        assert_eq!(
            power(Err(NvmlError::Unknown), || panic!("not read")),
            PowerReading::Energy(Err(Missing::Failed))
        );
    }

    #[test]
    fn d20_each_kind_of_failure_to_start_is_told_apart() {
        assert_eq!(failure(&NvmlError::LibraryNotFound), GpuFailure::NoLibrary);
        assert_eq!(
            failure(&NvmlError::DriverNotLoaded),
            GpuFailure::DriverNotLoaded
        );
        assert_eq!(
            failure(&NvmlError::LibRmVersionMismatch),
            GpuFailure::VersionMismatch
        );
        assert_eq!(failure(&NvmlError::NoPermission), GpuFailure::NoPermission);
        assert_eq!(failure(&NvmlError::InvalidArg), GpuFailure::Other);
        assert_eq!(failure(&NvmlError::GpuLost), GpuFailure::Lost);
        assert_eq!(start_failure(&NvmlError::GpuLost), GpuFailure::Other);
        assert_eq!(start_failure(&NvmlError::Unknown), GpuFailure::Other);
        assert_eq!(
            start_failure(&NvmlError::DriverNotLoaded),
            GpuFailure::DriverNotLoaded
        );
    }
}
