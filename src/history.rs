//! The last hour of readings (D16), with what is computed from them: the
//! stretches with no readings (D17), the rates, the averaged GPU power, the
//! share of time over the power threshold (D30), and the points to draw.
//!
//! Everything computed at a reading depends only on the readings before it,
//! so it is computed once, when the reading is pushed, and kept beside it.

use std::collections::VecDeque;
use std::time::Duration;

use crate::reading::{CpuTimes, GpuState, Missing, PowerReading, Reading, Value};

/// How long readings are kept, whatever the refresh interval (D16). History
/// only chooses how much of it the graphs show.
pub const KEPT: Duration = Duration::from_secs(60 * 60);

/// GPU power is averaged over at least this, less half the refresh interval,
/// because the energy counter moves in steps of about 100 ms, so an average
/// over 200 ms would be off by up to half its value. The half interval absorbs
/// the few milliseconds by which readings drift, so that at a refresh of 2 s
/// or more the window starts at the previous reading.
pub const POWER_WINDOW: Duration = Duration::from_secs(2);

pub struct Entry {
    pub reading: Reading,
    pub derived: Derived,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Derived {
    /// The time of the previous reading, when a stretch with no readings
    /// (D17), such as a suspend, lies between it and this one.
    pub stretch_since: Option<Duration>,
    /// In percent.
    pub cpu_usage: Value<f64>,
    pub core_usage: Vec<Value<f64>>,
    /// Beside `reading.disks`, in bytes per second.
    pub disk_rates: Vec<DiskRates>,
    /// Beside `reading.networks`, in bytes per second.
    pub network_rates: Vec<NetworkRates>,
    /// Beside the GPUs of `reading.gpu`, when it is running.
    pub gpu_power: Vec<Value<GpuPower>>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DiskRates {
    pub read: Value<f64>,
    pub written: Value<f64>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NetworkRates {
    pub received: Value<f64>,
    pub transmitted: Value<f64>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GpuPower {
    pub watts: f64,
    /// How long the energy counter's average spans, or `None` when the card
    /// has no energy counter and this is one instantaneous reading.
    pub averaged_over: Option<Duration>,
}

#[derive(Default)]
pub struct History {
    entries: VecDeque<Entry>,
}

impl History {
    pub fn push(&mut self, reading: Reading) {
        let derived = self.derive(&reading);
        self.entries.push_back(Entry { reading, derived });
        let newest = self.entries.back().unwrap().reading.time;
        while self
            .entries
            .front()
            .is_some_and(|e| e.reading.time + KEPT < newest)
        {
            self.entries.pop_front();
        }
    }

    #[cfg(test)]
    pub fn entries(&self) -> &VecDeque<Entry> {
        &self.entries
    }

    pub fn newest(&self) -> Option<&Entry> {
        self.entries.back()
    }

    /// The entries whose time lies within `span` of the newest.
    pub fn shown(&self, span: Duration) -> impl Iterator<Item = &Entry> + Clone {
        let start = self.shown_start(span);
        let first = self.entries.partition_point(|e| e.reading.time < start);
        self.entries.range(first..)
    }

    fn shown_start(&self, span: Duration) -> Duration {
        self.newest()
            .map_or(Duration::ZERO, |e| e.reading.time.saturating_sub(span))
    }

    fn derive(&self, reading: &Reading) -> Derived {
        let previous = self.entries.back().map(|e| &e.reading);
        let stretch_since = previous.filter(|p| is_stretch(p, reading)).map(|p| p.time);
        let elapsed = previous.map(|p| reading.time.saturating_sub(p.time).as_secs_f64());
        // A rate needs a previous reading on the same side of any stretch.
        let rate = |now: Value<u64>, before: Option<Value<u64>>| -> Value<f64> {
            let now = now?;
            let before = before.and_then(|b| b.ok()).ok_or(Missing::Waiting)?;
            let elapsed = elapsed.ok_or(Missing::Waiting)?;
            if stretch_since.is_some() || now < before || elapsed <= 0.0 {
                return Err(Missing::NoRate);
            }
            Ok((now - before) as f64 / elapsed)
        };
        let usage = |now: Option<&CpuTimes>, before: Option<&CpuTimes>| -> Value<f64> {
            let now = now.ok_or(Missing::Failed)?;
            let before = before.ok_or(Missing::Waiting)?;
            if stretch_since.is_some() || now.busy < before.busy || now.all <= before.all {
                return Err(Missing::NoRate);
            }
            Ok((now.busy - before.busy) as f64 / (now.all - before.all) as f64 * 100.0)
        };

        let cpu_before = previous.and_then(|p| p.cpu.as_ref().ok());
        let (cpu_usage, core_usage) = match &reading.cpu {
            Err(missing) => (Err(*missing), Vec::new()),
            Ok(cpu) => (
                usage(Some(&cpu.total), cpu_before.map(|c| &c.total)),
                (cpu.cores.iter().enumerate())
                    .map(|(i, core)| usage(Some(core), cpu_before.and_then(|c| c.cores.get(i))))
                    .collect(),
            ),
        };

        let disk_rates = (reading.disks.iter())
            .map(|disk| {
                let before =
                    previous.and_then(|p| p.disks.iter().find(|d| d.device == disk.device));
                DiskRates {
                    read: rate(disk.read, before.map(|d| d.read)),
                    written: rate(disk.written, before.map(|d| d.written)),
                }
            })
            .collect();

        let network_rates = (reading.networks.iter())
            .map(|net| {
                let before =
                    previous.and_then(|p| p.networks.iter().find(|n| n.interface == net.interface));
                NetworkRates {
                    received: rate(Ok(net.received), before.map(|n| Ok(n.received))),
                    transmitted: rate(Ok(net.transmitted), before.map(|n| Ok(n.transmitted))),
                }
            })
            .collect();

        let gpu_power = match &reading.gpu {
            GpuState::Running(gpus) => (gpus.iter())
                .map(|gpu| self.gpu_power(reading, &gpu.id, gpu.power, stretch_since.is_some()))
                .collect(),
            GpuState::NotStarted { .. } => Vec::new(),
        };

        Derived {
            stretch_since,
            cpu_usage,
            core_usage,
            disk_rates,
            network_rates,
            gpu_power,
        }
    }

    /// The GPU's power at `reading`, averaged from the energy counter between
    /// it and the newest earlier reading with an energy value that is old
    /// enough (see `POWER_WINDOW`).
    fn gpu_power(
        &self,
        reading: &Reading,
        id: &str,
        power: PowerReading,
        stretch_before: bool,
    ) -> Value<GpuPower> {
        let energy = match power {
            PowerReading::Instant(watts) => {
                return watts.map(|watts| GpuPower {
                    watts,
                    averaged_over: None,
                });
            }
            PowerReading::Energy(energy) => energy?,
        };
        let needed = POWER_WINDOW.saturating_sub(reading.interval / 2);
        let mut later_energy = energy;
        let mut crosses_stretch = stretch_before;
        for entry in self.entries.iter().rev() {
            if crosses_stretch {
                return Err(Missing::NoRate);
            }
            crosses_stretch = entry.derived.stretch_since.is_some();
            let earlier = match &entry.reading.gpu {
                GpuState::Running(gpus) => gpus.iter().find(|g| g.id == id),
                GpuState::NotStarted { .. } => None,
            };
            let Some(PowerReading::Energy(Ok(earlier_energy))) = earlier.map(|g| g.power) else {
                continue;
            };
            if earlier_energy > later_energy {
                return Err(Missing::NoRate);
            }
            later_energy = earlier_energy;
            let span = reading.time.saturating_sub(entry.reading.time);
            if span >= needed && !span.is_zero() {
                return Ok(GpuPower {
                    watts: (energy - earlier_energy) as f64 / 1000.0 / span.as_secs_f64(),
                    averaged_over: Some(span),
                });
            }
        }
        Err(Missing::Waiting)
    }

    /// The share of time, in percent, during which the GPU's power was at or
    /// above `threshold` watts, within the shown history (D30). Each interval
    /// is judged by the power at its later reading and counts for its length
    /// in time. An interval with no power counts on neither side.
    pub fn share_over(&self, gpu_id: &str, threshold: f64, span: Duration) -> Value<f64> {
        let start = self.shown_start(span);
        let mut over = 0.0;
        let mut counted = 0.0;
        let mut previous_time = None;
        for entry in &self.entries {
            let time = entry.reading.time;
            let interval = previous_time
                .replace(time)
                .map(|p| (time - p).as_secs_f64());
            let (Some(interval), true) = (interval, time >= start) else {
                continue;
            };
            if let Some(Ok(power)) = gpu_power_of(entry, gpu_id) {
                counted += interval;
                if power.watts >= threshold {
                    over += interval;
                }
            }
        }
        if counted > 0.0 {
            return Ok(over / counted * 100.0);
        }
        match self.newest().and_then(|e| gpu_power_of(e, gpu_id)) {
            Some(Err(missing)) => Err(missing),
            _ => Err(Missing::Waiting),
        }
    }

    /// The default threshold (D30): 95% of the enforced limit in the latest
    /// reading that has one, in whole watts, so that it follows a change of
    /// the limit and one failed read of the limit does not drop it.
    pub fn default_threshold(&self, gpu_id: &str) -> Option<f64> {
        self.entries
            .iter()
            .rev()
            .find_map(|entry| match &entry.reading.gpu {
                GpuState::Running(gpus) => gpus
                    .iter()
                    .find(|g| g.id == gpu_id)
                    .and_then(|g| g.power_limit.ok())
                    .map(|limit| (limit * 0.95).round()),
                GpuState::NotStarted { .. } => None,
            })
    }

    /// The stretches with no readings within the shown history, as
    /// (start, end) in seconds before the newest reading.
    pub fn stretches(&self, span: Duration) -> Vec<(f64, f64)> {
        let Some(newest) = self.newest().map(|e| e.reading.time) else {
            return Vec::new();
        };
        let start = self.shown_start(span);
        let since = |t: Duration| t.as_secs_f64() - newest.as_secs_f64();
        self.shown(span)
            .filter_map(|e| {
                let from = e.derived.stretch_since?;
                Some((since(from.max(start)), since(e.reading.time)))
            })
            .collect()
    }

    /// One series to draw, within the shown history, as lines broken at each
    /// stretch with no readings and each missing value, and reduced to
    /// `buckets` time buckets across the span. x is in seconds before the
    /// newest reading.
    pub fn series(
        &self,
        span: Duration,
        buckets: usize,
        value: impl Fn(&Entry) -> Option<Value<f64>>,
    ) -> Vec<Vec<[f64; 2]>> {
        let Some(newest) = self.newest().map(|e| e.reading.time.as_secs_f64()) else {
            return Vec::new();
        };
        let mut lines = Vec::new();
        let mut line: Vec<[f64; 2]> = Vec::new();
        for entry in self.shown(span) {
            if entry.derived.stretch_since.is_some() && !line.is_empty() {
                lines.push(std::mem::take(&mut line));
            }
            match value(entry) {
                Some(Ok(y)) => line.push([entry.reading.time.as_secs_f64() - newest, y]),
                _ if !line.is_empty() => lines.push(std::mem::take(&mut line)),
                _ => {}
            }
        }
        if !line.is_empty() {
            lines.push(line);
        }
        let width = span.as_secs_f64() / buckets.max(1) as f64;
        (lines.iter())
            .map(|line| reduce(line, -span.as_secs_f64(), width))
            .collect()
    }
}

/// The power computed at `entry` for the GPU `gpu_id`, if the entry has it.
pub fn gpu_power_of(entry: &Entry, gpu_id: &str) -> Option<Value<GpuPower>> {
    let GpuState::Running(gpus) = &entry.reading.gpu else {
        return None;
    };
    let index = gpus.iter().position(|g| g.id == gpu_id)?;
    entry.derived.gpu_power.get(index).copied()
}

/// Two neighboring readings further apart than twice the larger of their
/// refresh intervals have a stretch with no readings between them (D17).
/// Taking the larger keeps a switch of interval, either way, from looking
/// like one.
fn is_stretch(earlier: &Reading, later: &Reading) -> bool {
    let limit = 2 * earlier.interval.max(later.interval);
    later.time.saturating_sub(earlier.time) > limit
}

/// Reduces a line to its smallest and largest point in each time bucket of
/// `width` seconds counted from `start`, in the order they occur, so that
/// readings at mixed intervals draw evenly and a short spike still shows.
pub fn reduce(points: &[[f64; 2]], start: f64, width: f64) -> Vec<[f64; 2]> {
    let mut reduced = Vec::new();
    let bucket_of = |p: &[f64; 2]| ((p[0] - start) / width).floor() as i64;
    let mut i = 0;
    while i < points.len() {
        let bucket = bucket_of(&points[i]);
        let mut end = i;
        while end < points.len() && bucket_of(&points[end]) == bucket {
            end += 1;
        }
        let slice = &points[i..end];
        let lowest = (0..slice.len())
            .min_by(|&a, &b| slice[a][1].total_cmp(&slice[b][1]))
            .unwrap();
        let highest = (0..slice.len())
            .max_by(|&a, &b| slice[a][1].total_cmp(&slice[b][1]))
            .unwrap();
        reduced.push(slice[lowest.min(highest)]);
        if lowest != highest {
            reduced.push(slice[lowest.max(highest)]);
        }
        i = end;
    }
    reduced
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::reading::{CpuStat, DiskReading, GpuFailure, GpuReading, NetworkReading};

    pub fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    /// A reading with nothing in it but its time and interval.
    pub fn bare(time: Duration, interval: Duration) -> Reading {
        Reading {
            time,
            interval,
            cpu: Err(Missing::NotReported),
            cpu_frequency: Err(Missing::NotReported),
            cpu_temperature: Err(Missing::NotReported),
            memory: Err(Missing::NotReported),
            disks: Vec::new(),
            networks: Vec::new(),
            gpu: GpuState::NotStarted {
                failure: GpuFailure::NoLibrary,
                cards: Vec::new(),
            },
        }
    }

    pub fn gpu(id: &str, power: PowerReading, limit: Value<f64>) -> GpuReading {
        GpuReading {
            id: id.to_string(),
            name: "Test GPU".to_string(),
            failure: None,
            utilization: Ok(50.0),
            memory_used: Ok(1 << 30),
            memory_total: Ok(8 << 30),
            power,
            power_limit: limit,
            power_limit_range: Ok((92.5, 210.0)),
            temperature: Ok(60.0),
            fans: Ok(vec![Ok(40.0), Ok(41.0)]),
            clock: Ok(1500.0),
            max_clock: Ok(2205.0),
        }
    }

    /// A reading of one GPU whose energy counter reads `energy` millijoules.
    fn with_energy(time: Duration, interval: Duration, energy: Value<u64>) -> Reading {
        Reading {
            gpu: GpuState::Running(vec![gpu("A", PowerReading::Energy(energy), Ok(185.0))]),
            ..bare(time, interval)
        }
    }

    /// Readings of one GPU drawing `watts(t)` steadily, at the given times.
    fn steady(times: &[(Duration, Duration)], watts: impl Fn(f64) -> f64) -> History {
        let mut history = History::default();
        let mut energy = 1_000_000.0;
        let mut last = None;
        for &(time, interval) in times {
            if let Some(last) = last {
                energy += watts(time.as_secs_f64()) * (time - last).as_secs_f64() * 1000.0;
            }
            last = Some(time);
            history.push(with_energy(time, interval, Ok(energy.round() as u64)));
        }
        history
    }

    fn every(interval: Duration, count: u32, from: Duration) -> Vec<(Duration, Duration)> {
        (0..count)
            .map(|i| (from + interval * i, interval))
            .collect()
    }

    fn power(history: &History) -> Value<GpuPower> {
        history.newest().unwrap().derived.gpu_power[0]
    }

    fn all_powers(history: &History) -> Vec<Value<GpuPower>> {
        history
            .entries()
            .iter()
            .map(|e| e.derived.gpu_power[0])
            .collect()
    }

    #[test]
    fn d16_history_keeps_an_hour_and_drops_older() {
        let mut history = History::default();
        for i in 0..=(3600 * 5 + 50) {
            history.push(bare(ms(200) * i, ms(200)));
        }
        let entries = history.entries();
        let span = entries.back().unwrap().reading.time - entries.front().unwrap().reading.time;
        assert_eq!(span, KEPT);
        assert_eq!(entries.len(), 18_001);
    }

    #[test]
    fn d16_switching_history_keeps_readings() {
        let mut history = History::default();
        for i in 0..3600 {
            history.push(bare(Duration::from_secs(i), Duration::from_secs(1)));
        }
        let times = |span: Duration| -> Vec<Duration> {
            history.shown(span).map(|e| e.reading.time).collect()
        };
        let fifteen = times(Duration::from_secs(15 * 60));
        assert_eq!(fifteen.len(), 15 * 60 + 1);
        assert_eq!(times(Duration::from_secs(5 * 60)).len(), 5 * 60 + 1);
        assert_eq!(times(Duration::from_secs(15 * 60)), fifteen);
    }

    #[test]
    fn d17_readings_further_apart_than_twice_the_larger_interval_have_a_stretch() {
        let mut history = History::default();
        history.push(bare(ms(0), ms(500)));
        history.push(bare(ms(1000), ms(500)));
        history.push(bare(ms(2001), ms(500)));
        let stretches: Vec<_> = history
            .entries()
            .iter()
            .map(|e| e.derived.stretch_since)
            .collect();
        assert_eq!(stretches, [None, None, Some(ms(1000))]);
        assert_eq!(history.stretches(Duration::from_secs(60)), [(-1.001, 0.0)]);
    }

    #[test]
    fn d17_no_stretch_at_a_switch_of_interval_either_way() {
        // From 5 s to 200 ms: the first reading taken at 200 ms comes at the
        // deadline set before the switch, 5 s after the last one.
        let mut times = every(Duration::from_secs(5), 4, ms(0));
        times.extend(every(ms(200), 20, ms(20_000)));
        // And back to 5 s: the first reading at 5 s comes 5 s after the last
        // one at 200 ms.
        let last = times.last().unwrap().0;
        times.extend(every(
            Duration::from_secs(5),
            4,
            last + Duration::from_secs(5),
        ));
        let mut history = History::default();
        for (time, interval) in times {
            history.push(bare(time, interval));
        }
        assert!(
            history
                .entries()
                .iter()
                .all(|e| e.derived.stretch_since.is_none())
        );
    }

    #[test]
    fn d17_an_interval_across_a_stretch_has_no_rate_and_no_share() {
        let disk = |read: u64| DiskReading {
            device: "/dev/sda1".into(),
            mount_points: vec!["/".into()],
            size: 100,
            available: 50,
            read: Ok(read),
            written: Ok(read),
        };
        let mut history = History::default();
        let mut push = |time: Duration, read: u64, energy: u64| {
            let mut reading = with_energy(time, ms(500), Ok(energy));
            reading.disks = vec![disk(read)];
            reading.networks = vec![NetworkReading {
                interface: "enp3s0".into(),
                received: read,
                transmitted: read,
            }];
            reading.cpu = Ok(CpuStat {
                total: CpuTimes {
                    busy: read,
                    all: read * 2,
                },
                cores: vec![],
            });
            history.push(reading);
        };
        // 200 W for 3 s, then ten minutes suspended, then 200 W for 0.5 s.
        // (After an hour, the readings before it would be older than KEPT.)
        for i in 0..=6 {
            push(ms(500) * i, 1000 * i as u64, 100_000 * i as u64);
        }
        push(ms(3000 + 600_000), 1_000_000, 700_000);
        let after = &history.newest().unwrap().derived;
        assert_eq!(after.disk_rates[0].read, Err(Missing::NoRate));
        assert_eq!(after.network_rates[0].received, Err(Missing::NoRate));
        assert_eq!(after.cpu_usage, Err(Missing::NoRate));
        assert_eq!(after.gpu_power[0], Err(Missing::NoRate));
        // The ten minutes count on neither side: every interval before them was over.
        assert_eq!(history.share_over("A", 150.0, KEPT), Ok(100.0));
    }

    #[test]
    fn a_counter_that_goes_backwards_gives_no_rate() {
        let mut history = History::default();
        history.push(with_energy(ms(0), ms(500), Ok(5_000_000)));
        for i in 1..=8 {
            history.push(with_energy(ms(500) * i, ms(500), Ok(1000 * i as u64)));
        }
        let powers = all_powers(&history);
        // The window of each reading until 2 s after the driver reloaded
        // reaches back past the reload.
        assert!(
            powers[1..=3]
                .iter()
                .all(|p| *p == Err(Missing::Waiting) || *p == Err(Missing::NoRate))
        );
        assert_eq!(powers[4], Err(Missing::NoRate));
        assert!(powers[5].is_ok());

        let mut disks = History::default();
        for (i, read) in [100, 200, 50, 80].into_iter().enumerate() {
            let mut reading = bare(ms(500) * i as u32, ms(500));
            reading.disks = vec![DiskReading {
                device: "/dev/sda1".into(),
                mount_points: vec![],
                size: 1,
                available: 1,
                read: Ok(read),
                written: Ok(read),
            }];
            disks.push(reading);
        }
        let rates: Vec<_> = disks
            .entries()
            .iter()
            .map(|e| e.derived.disk_rates[0].read)
            .collect();
        assert_eq!(
            rates,
            [
                Err(Missing::Waiting),
                Ok(200.0),
                Err(Missing::NoRate),
                Ok(60.0)
            ]
        );
    }

    #[test]
    fn reducing_18000_points_keeps_a_single_spike() {
        let mut points: Vec<[f64; 2]> = (0..18_000)
            .map(|i| [i as f64 * 0.2 - 3600.0, 10.0])
            .collect();
        points[12_345][1] = 99.0;
        let reduced = reduce(&points, -3600.0, 3600.0 / 800.0);
        assert!(reduced.len() <= 1600);
        assert!(reduced.iter().any(|p| p[1] == 99.0));
        assert!(reduced.windows(2).all(|w| w[0][0] < w[1][0]));
    }

    #[test]
    fn readings_at_mixed_intervals_reduce_to_evenly_spaced_buckets() {
        // 30 minutes at 200 ms, then 30 minutes at 5 s.
        let mut points: Vec<[f64; 2]> = (0..9000)
            .map(|i| [i as f64 * 0.2, (i % 7) as f64])
            .collect();
        points.extend((0..360).map(|i| [1800.0 + i as f64 * 5.0, (i % 7) as f64]));
        let width = 10.0;
        let reduced = reduce(&points, 0.0, width);
        let mut per_bucket = std::collections::HashMap::<i64, usize>::new();
        for p in &reduced {
            *per_bucket.entry((p[0] / width).floor() as i64).or_default() += 1;
        }
        assert_eq!(per_bucket.len(), 360);
        assert!(per_bucket.values().all(|&n| n == 2));
    }

    #[test]
    fn d30_with_no_power_value_the_share_is_missing_with_a_reason() {
        let mut history = History::default();
        history.push(with_energy(ms(0), ms(500), Ok(0)));
        history.push(with_energy(ms(500), ms(500), Ok(100)));
        assert_eq!(history.share_over("A", 100.0, KEPT), Err(Missing::Waiting));
        history.push(with_energy(ms(1000), ms(500), Err(Missing::Failed)));
        assert_eq!(history.share_over("A", 100.0, KEPT), Err(Missing::Failed));
        assert_eq!(
            History::default().share_over("A", 100.0, KEPT),
            Err(Missing::Waiting)
        );
    }

    #[test]
    fn d30_share_over_the_threshold() {
        let times = every(ms(500), 61, ms(0));
        // 100 W for the first 15 s, then 200 W.
        let history = steady(&times, |t| if t <= 15.0 { 100.0 } else { 200.0 });
        let all = KEPT;
        assert_eq!(history.share_over("A", 50.0, all), Ok(100.0));
        assert_eq!(history.share_over("A", 250.0, all), Ok(0.0));
        let share = history.share_over("A", 150.0, all).unwrap();
        // The readings before 2 s have no power value and count for nothing,
        // so 28.5 s count. The 2 s windows average 150 W or more from 16 s on.
        assert!((share - 14.5 / 28.5 * 100.0).abs() < 1e-9, "{share}");
        // The interval at 16 s averages exactly 150 W, which counts as over.
        let just_above = history.share_over("A", 150.001, all).unwrap();
        assert!((share - just_above - 0.5 / 28.5 * 100.0).abs() < 1e-9);
        // Changing the threshold changes the share at once.
        assert_ne!(history.share_over("A", 99.0, all), Ok(share));
    }

    #[test]
    fn d30_a_5_s_interval_weighs_as_much_as_twenty_five_of_200_ms() {
        // A GPU reading instantaneous power, so that each reading has its own.
        let mut history = History::default();
        let push = |history: &mut History, time: Duration, interval: Duration, watts: f64| {
            history.push(Reading {
                gpu: GpuState::Running(vec![gpu("A", PowerReading::Instant(Ok(watts)), Ok(185.0))]),
                ..bare(time, interval)
            });
        };
        push(&mut history, ms(0), ms(200), 100.0);
        for i in 1..=25 {
            push(&mut history, ms(200) * i, ms(200), 100.0);
        }
        push(&mut history, ms(5000 + 5000), Duration::from_secs(5), 200.0);
        let share = history.share_over("A", 150.0, KEPT).unwrap();
        assert!((share - 50.0).abs() < 1e-9, "{share}");
    }

    #[test]
    fn d30_default_threshold_follows_the_enforced_limit() {
        let mut history = History::default();
        let push = |history: &mut History, time: u64, limit: Value<f64>| {
            history.push(Reading {
                gpu: GpuState::Running(vec![gpu("A", PowerReading::Energy(Ok(time)), limit)]),
                ..bare(ms(time), ms(500))
            });
        };
        assert_eq!(history.default_threshold("A"), None);
        push(&mut history, 0, Ok(185.0));
        assert_eq!(history.default_threshold("A"), Some(176.0));
        push(&mut history, 500, Err(Missing::Failed));
        assert_eq!(history.default_threshold("A"), Some(176.0));
        push(&mut history, 1000, Ok(200.0));
        assert_eq!(history.default_threshold("A"), Some(190.0));
        assert_eq!(history.default_threshold("B"), None);
        push(&mut history, 1500, Err(Missing::NotReported));
        assert_eq!(history.default_threshold("A"), Some(190.0));
    }

    #[test]
    fn d30_a_gpu_that_reports_no_limit_has_no_default_threshold() {
        let mut history = History::default();
        history.push(Reading {
            gpu: GpuState::Running(vec![gpu(
                "A",
                PowerReading::Instant(Ok(10.0)),
                Err(Missing::NotReported),
            )]),
            ..bare(ms(0), ms(500))
        });
        assert_eq!(history.default_threshold("A"), None);
    }

    /// Asserts the window of every reading from the fourth second on, with
    /// readings drifting a few milliseconds early and late.
    fn assert_windows(interval: Duration, low: f64, high: f64) {
        let drift = [0i64, 3, -4, 2, -1, 4, -3];
        let times: Vec<_> = (0..120u32)
            .map(|i| {
                let t = (interval * i).as_millis() as i64 + drift[i as usize % drift.len()];
                (ms(t.max(0) as u64), interval)
            })
            .collect();
        let history = steady(&times, |_| 150.0);
        for entry in history
            .entries()
            .iter()
            .filter(|e| e.reading.time >= Duration::from_secs(4) + interval)
        {
            let power = entry.derived.gpu_power[0].unwrap();
            let window = power.averaged_over.unwrap().as_secs_f64();
            assert!(
                window >= low && window <= high,
                "{interval:?}: window {window}"
            );
            assert!((power.watts - 150.0).abs() < 0.5, "{}", power.watts);
        }
    }

    #[test]
    fn d1_gpu_power_is_averaged_over_2_s_less_half_the_refresh_interval() {
        assert_windows(ms(200), 1.9, 2.2);
        assert_windows(ms(500), 1.75, 2.1);
        // At 2 s and 5 s, over the previous interval.
        assert_windows(Duration::from_secs(2), 1.99, 2.01);
        assert_windows(Duration::from_secs(5), 4.99, 5.01);
        assert_windows(Duration::from_secs(1), 1.99, 2.01);
    }

    #[test]
    fn d1_gpu_power_waits_for_an_old_enough_reading_and_returns_after_a_stretch() {
        let mut times = every(ms(500), 10, ms(0));
        times.extend(every(ms(500), 10, Duration::from_secs(100)));
        let history = steady(&times, |_| 150.0);
        let powers = all_powers(&history);
        assert!(powers[..4].iter().all(|p| *p == Err(Missing::Waiting)));
        assert!(powers[4..10].iter().all(Result::is_ok));
        assert!(powers[10..14].iter().all(|p| *p == Err(Missing::NoRate)));
        assert!(powers[14..].iter().all(Result::is_ok));
        assert_eq!(power(&history).unwrap().averaged_over, Some(ms(2000)));
    }

    #[test]
    fn d1_gpu_power_windows_grow_across_a_switch_from_5_s_to_200_ms() {
        let mut times = vec![
            (ms(0), Duration::from_secs(5)),
            (ms(5000), Duration::from_secs(5)),
        ];
        times.extend(every(ms(200), 30, ms(5200)));
        let history = steady(&times, |_| 150.0);
        let windows: Vec<u128> = (history.entries().iter().skip(2))
            .map(|e| {
                e.derived.gpu_power[0]
                    .unwrap()
                    .averaged_over
                    .unwrap()
                    .as_millis()
            })
            .collect();
        // Until a reading after the switch is 1.9 s old, each window starts at 0 s.
        let growing: Vec<u128> = (0..9).map(|i| 5200 + 200 * i).collect();
        assert_eq!(windows[..9], growing[..]);
        assert!(windows[9..].iter().all(|&w| w == 2000));
    }

    #[test]
    fn d1_a_reading_whose_energy_read_failed_is_skipped_as_a_start() {
        let mut history = History::default();
        history.push(with_energy(ms(0), ms(500), Ok(0)));
        history.push(with_energy(ms(500), ms(500), Err(Missing::Failed)));
        history.push(with_energy(ms(1000), ms(500), Ok(300_000)));
        assert_eq!(power(&history), Err(Missing::Waiting));
        history.push(with_energy(ms(1500), ms(500), Err(Missing::Failed)));
        assert_eq!(power(&history), Err(Missing::Failed));
        history.push(with_energy(ms(2000), ms(500), Ok(600_000)));
        assert_eq!(
            power(&history),
            Ok(GpuPower {
                watts: 300.0,
                averaged_over: Some(ms(2000))
            })
        );
    }

    #[test]
    fn d1_on_a_card_without_the_energy_counter_power_is_instantaneous() {
        let mut history = History::default();
        for i in 0..3 {
            history.push(Reading {
                gpu: GpuState::Running(vec![gpu(
                    "A",
                    PowerReading::Instant(Ok([170.0, 175.0, 190.0][i as usize])),
                    Ok(185.0),
                )]),
                ..bare(ms(500) * i, ms(500))
            });
        }
        assert_eq!(
            power(&history),
            Ok(GpuPower {
                watts: 190.0,
                averaged_over: None
            })
        );
        assert_eq!(history.share_over("A", 176.0, KEPT), Ok(50.0));
    }

    #[test]
    fn series_break_at_stretches_and_missing_values() {
        let mut history = History::default();
        let readings = [
            (0, Ok(1)),
            (500, Ok(2)),
            (1000, Err(Missing::Failed)),
            (1500, Ok(3)),
            (9000, Ok(4)),
        ];
        for (time, read) in readings {
            let mut reading = bare(ms(time), ms(500));
            reading.memory = read.map(|used| crate::reading::Memory {
                used,
                total: 10,
                swap_used: 0,
                swap_total: 0,
            });
            history.push(reading);
        }
        let lines = history.series(Duration::from_secs(60), 600, |e| {
            Some(e.reading.memory.map(|m| m.used as f64))
        });
        assert_eq!(
            lines,
            [
                vec![[-9.0, 1.0], [-8.5, 2.0]],
                vec![[-7.5, 3.0]],
                vec![[0.0, 4.0]]
            ]
        );
    }
}
