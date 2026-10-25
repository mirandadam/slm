//! The window's tests, and the scenarios the copy step photographs (D19),
//! each with fake readings, so that every state can be reached.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use egui_kittest::Harness;
use egui_kittest::kittest::{By, NodeT, Queryable};

use super::*;
use crate::history::tests::{bare, gpu};
use crate::reading::{
    CpuStat, CpuTimes, DiskReading, GpuFailure, Memory, NetworkReading, PowerReading, Reading,
};

#[derive(Default)]
struct MemoryStorage(HashMap<String, String>);

impl eframe::Storage for MemoryStorage {
    fn get_string(&self, key: &str) -> Option<String> {
        self.0.get(key).cloned()
    }
    fn set_string(&mut self, key: &str, value: String) {
        self.0.insert(key.to_string(), value);
    }
    fn remove_string(&mut self, key: &str) {
        self.0.remove(key);
    }
    fn flush(&mut self) {}
}

/// A machine whose load moves in slow waves, read every `interval`.
struct Machine {
    time: Duration,
    interval: Duration,
    cpu: CpuStat,
    energy: f64,
    disk: u64,
    network: u64,
    /// The GPU as each reading starts it, before its load is set.
    gpu: GpuReading,
}

const GPU_ID: &str = "GPU-e2e0577a";

impl Machine {
    fn new(interval: Duration) -> Self {
        let core = CpuTimes { busy: 0, all: 0 };
        Self {
            time: Duration::from_secs(1000),
            interval,
            cpu: CpuStat {
                total: core,
                cores: vec![core; 12],
            },
            energy: 1e9,
            disk: 1 << 30,
            network: 1 << 30,
            gpu: GpuReading {
                name: "NVIDIA GeForce RTX 2070".into(),
                ..gpu(GPU_ID, PowerReading::Energy(Ok(0)), Ok(185.0))
            },
        }
    }

    fn wave(&self, period: f64, low: f64, high: f64) -> f64 {
        let t = self.time.as_secs_f64();
        low + (high - low) * (0.5 + 0.5 * (t / period).sin())
    }

    fn reading(&mut self) -> Reading {
        self.time += self.interval;
        let seconds = self.interval.as_secs_f64();
        let ticks = (seconds * 100.0) as u64;
        let usage = self.wave(23.0, 0.1, 0.7);
        for (i, core) in self.cpu.cores.iter_mut().enumerate() {
            core.all += ticks;
            core.busy += (ticks as f64 * (usage + 0.02 * i as f64).min(1.0)) as u64;
        }
        self.cpu.total = CpuTimes {
            busy: self.cpu.cores.iter().map(|c| c.busy).sum(),
            all: self.cpu.cores.iter().map(|c| c.all).sum(),
        };
        let watts = self.wave(40.0, 90.0, 185.0);
        self.energy += watts * seconds * 1000.0;
        self.disk += (self.wave(17.0, 0.0, 40e6) * seconds) as u64;
        self.network += (self.wave(29.0, 2e3, 900e3) * seconds) as u64;
        let mut gpu = self.gpu.clone();
        if gpu.failure.is_none() {
            gpu.power = match gpu.power {
                PowerReading::Energy(_) => PowerReading::Energy(Ok(self.energy as u64)),
                PowerReading::Instant(_) => PowerReading::Instant(Ok(watts)),
            };
            gpu.utilization = Ok(self.wave(40.0, 30.0, 99.0).round());
            gpu.memory_used = Ok((self.wave(90.0, 1.5, 6.0) * (1u64 << 30) as f64) as u64);
        }
        Reading {
            cpu: Ok(self.cpu.clone()),
            cpu_frequency: Ok(self.wave(31.0, 2200.0, 4100.0).round()),
            cpu_temperature: Ok(self.wave(50.0, 45.0, 78.0).round()),
            memory: Ok(Memory {
                used: 6_228_017_152,
                total: 16_715_452_416,
                swap_used: 1_504_420_352,
                swap_total: 20_105_392_128,
            }),
            disks: vec![DiskReading {
                device: "/dev/nvme0n1p1".into(),
                mount_points: vec![PathBuf::from("/"), PathBuf::from("/home")],
                size: 479_999_295_488,
                available: 60_475_510_784,
                read: Ok(self.disk),
                written: Ok(self.disk / 3),
            }],
            networks: vec![NetworkReading {
                interface: "enp3s0".into(),
                received: self.network,
                transmitted: self.network / 5,
            }],
            gpu: GpuState::Running(vec![gpu]),
            ..bare(self.time, self.interval)
        }
    }

    fn run(&mut self, history: &mut History, count: u32) {
        for _ in 0..count {
            history.push(self.reading());
        }
    }
}

/// Five minutes of a machine read every 500 ms.
fn running() -> (Machine, History) {
    let mut machine = Machine::new(Duration::from_millis(500));
    let mut history = History::default();
    machine.run(&mut history, 600);
    (machine, history)
}

/// The app, on a clock that stands at its newest reading, so that it is live.
fn app(history: History, storage: Option<&dyn eframe::Storage>) -> App {
    let now = history.newest().map_or(Duration::ZERO, |e| e.reading.time);
    let mut app = App::new(
        storage,
        Arc::new(Mutex::new(history)),
        Arc::new(AtomicU64::new(0)),
    );
    app.clock = Box::new(move || now);
    app.opened = now;
    app
}

/// The window at its first width, tall enough that nothing scrolls.
fn open(app: App) -> Harness<'static, App> {
    open_sized(app, WINDOW_SIZE[0], 600.0)
}

fn open_sized(app: App, width: f32, height: f32) -> Harness<'static, App> {
    let mut harness = Harness::builder()
        .with_size(egui::vec2(width, height))
        .build_ui_state(|ui, app: &mut App| app.show(ui), app);
    harness.run_steps(4);
    harness
}

fn with_history(harness: &mut Harness<'_, App>, change: impl FnOnce(&mut History)) {
    change(&mut harness.state().history.lock().unwrap());
    harness.run_steps(4);
}

/// The texts of the tooltips of every missing value's mark, hovered in turn.
fn missing_tooltips(harness: &mut Harness<'_, App>) -> Vec<String> {
    let count = harness.get_all_by_label(text::MISSING).count();
    let reasons = [
        text::NOT_REPORTED,
        text::WAITING,
        text::NO_RATE,
        text::FAILED,
    ];
    (0..count)
        .map(|i| {
            harness
                .get_all_by_label(text::MISSING)
                .nth(i)
                .unwrap()
                .hover();
            harness.run_steps(4);
            let shown: Vec<&str> = (reasons.iter())
                .filter(|r| harness.query_by_label(r).is_some())
                .copied()
                .collect();
            assert_eq!(shown.len(), 1, "one reason per mark");
            shown[0].to_string()
        })
        .collect()
}

const GPU_NAME: &str = "NVIDIA GeForce RTX 2070";

/// Opens the settings' menu, unless it is open.
fn open_settings(harness: &mut Harness<'_, App>) {
    if harness.query_by_label(text::REFRESH).is_none() {
        harness.get_by_label(text::SETTINGS).click();
        harness.run_steps(4);
    }
}

/// The threshold's field of the GPU `name`, in the settings' menu.
fn threshold_of_gpu<'h>(harness: &'h Harness<'_, App>, name: &str) -> egui_kittest::Node<'h> {
    // The query keeps the label as long as the node, so a test leaks it.
    let label = fill(text::THRESHOLD_FOR, &[("name", name)]).leak();
    harness.get_by_role_and_label(egui::accesskit::Role::SpinButton, label)
}

fn threshold_field<'h>(harness: &'h Harness<'_, App>) -> egui_kittest::Node<'h> {
    threshold_of_gpu(harness, GPU_NAME)
}

#[test]
fn d18_a_value_not_reported_shows_its_mark_and_a_reported_0_shows_0() {
    let (mut machine, mut history) = running();
    machine.gpu.fans = Err(Missing::NotReported);
    machine.gpu.temperature = Ok(0.0);
    machine.run(&mut history, 1);
    let mut harness = open(app(history, None));
    harness.get_by_label(&celsius(0.0));
    assert_eq!(harness.get_all_by_label(text::MISSING).count(), 1);
    harness.get_by_label(text::FANS);
    assert_eq!(missing_tooltips(&mut harness), [text::NOT_REPORTED]);
}

#[test]
fn each_reason_a_value_is_missing_has_its_own_tooltip() {
    let (mut machine, mut history) = running();
    machine.gpu.fans = Err(Missing::NotReported);
    machine.gpu.temperature = Err(Missing::Failed);
    // A suspend, after which power and rates have no rate for 2 s.
    machine.time += Duration::from_secs(600);
    machine.run(&mut history, 1);
    let mut harness = open_sized(app(history, None), 1000.0, 1700.0);
    // A rate waits for a second reading of a disk that just appeared.
    with_history(&mut harness, |history| {
        let mut reading = machine.reading();
        let mut disk = reading.disks[0].clone();
        disk.device = "/dev/sda1".into();
        reading.disks.push(disk);
        history.push(reading);
    });
    let mut tooltips = missing_tooltips(&mut harness);
    tooltips.sort();
    tooltips.dedup();
    let mut expected = vec![
        text::NOT_REPORTED,
        text::WAITING,
        text::NO_RATE,
        text::FAILED,
    ];
    expected.sort();
    assert_eq!(tooltips, expected);
}

#[test]
fn d20_each_kind_of_failure_shows_its_own_sentence_and_never_the_error() {
    let failures = [
        (GpuFailure::NoLibrary, text::GPU_NO_DRIVER),
        (GpuFailure::DriverNotLoaded, text::GPU_DRIVER_NOT_RUNNING),
        (GpuFailure::VersionMismatch, text::GPU_VERSION_MISMATCH),
        (GpuFailure::NoPermission, text::GPU_NO_PERMISSION),
        (GpuFailure::Other, text::GPU_OTHER_FAILURE),
    ];
    for (failure, sentence) in failures {
        let mut history = History::default();
        history.push(Reading {
            gpu: GpuState::NotStarted {
                failure,
                cards: vec!["0000:07:00.0".into(), "0000:0a:00.0".into()],
            },
            ..bare(Duration::from_secs(1), Duration::from_millis(500))
        });
        let harness = open(app(history, None));
        // Each card is named, and the sentence they share is said once.
        assert_eq!(harness.get_all_by_label_contains("0000:").count(), 2);
        assert_eq!(harness.get_all_by_label(sentence).count(), 1, "{failure:?}");
        for (_, other) in failures.iter().filter(|(_, s)| *s != sentence) {
            assert!(harness.query_by_label(other).is_none());
        }
        for name in [
            "NoLibrary",
            "Libloading",
            "NotLoaded",
            "Mismatch",
            "Permission",
            "Nvml",
        ] {
            assert!(harness.query_by_label_contains(name).is_none(), "{name}");
        }
    }
}

#[test]
fn gpus_of_the_same_model_are_numbered() {
    let (mut machine, mut history) = running();
    let mut reading = machine.reading();
    let GpuState::Running(gpus) = &mut reading.gpu else {
        unreachable!()
    };
    let second = GpuReading {
        id: "GPU-second".into(),
        ..gpus[0].clone()
    };
    gpus.push(second);
    history.push(reading);
    let mut harness = open_sized(app(history, None), WINDOW_SIZE[0], 900.0);
    let name = GPU_NAME;
    for n in ["1", "2"] {
        let line = fill(
            text::NUMBERED_LINE,
            &[("name", text::UTILIZATION), ("n", n)],
        );
        harness.get_by_label(&line);
    }
    open_settings(&mut harness);
    for n in ["1", "2"] {
        harness.get_by_label(&fill(text::GPU_NUMBERED, &[("name", name), ("n", n)]));
    }
    assert!(
        harness
            .query_by_label(&fill(text::GPU, &[("name", name)]))
            .is_none()
    );
}

#[test]
fn with_several_gpus_each_numbered_line_names_its_card() {
    let (mut machine, mut history) = running();
    let mut reading = machine.reading();
    let GpuState::Running(gpus) = &mut reading.gpu else {
        unreachable!()
    };
    let second = GpuReading {
        id: "GPU-second".into(),
        name: "Second".into(),
        ..gpus[0].clone()
    };
    gpus.push(second);
    history.push(reading);
    let mut harness = open(app(history, None));
    for name in [text::UTILIZATION, text::VRAM, text::POWER] {
        harness
            .get_by_label(&fill(text::NUMBERED_LINE, &[("name", name), ("n", "2")]))
            .hover();
        harness.run_steps(4);
        harness.get_by_label("Second");
    }
}

#[test]
fn half_a_rate_pair_missing_says_why() {
    let (mut machine, mut history) = running();
    for _ in 0..2 {
        let mut reading = machine.reading();
        reading.disks[0].written = Err(Missing::Failed);
        history.push(reading);
    }
    let mut harness = open(app(history, None));
    harness
        .get_all_by_label_contains(text::MISSING)
        .find(|node| {
            // The pair, not a mark on its own.
            format!(
                "{:?}{:?}",
                node.accesskit_node().label(),
                node.accesskit_node().value()
            )
            .contains(&text::PAIR.replace("{first}", "").replace("{second}", ""))
        })
        .unwrap()
        .hover();
    harness.run_steps(4);
    harness.get_by_label_contains(text::FAILED);
}

#[test]
fn the_logical_cpus_follow_their_own_box() {
    let (_, history) = running();
    let mut harness = open(app(history, None));
    uncheck(&mut harness, text::USAGE, 0);
    harness.get_by_label_contains(text::CORES);
}

#[test]
fn a_machine_with_no_swap_says_so_rather_than_that_it_is_not_reported() {
    let (mut machine, mut history) = running();
    let mut reading = machine.reading();
    reading.memory = reading.memory.map(|m| Memory {
        swap_used: 0,
        swap_total: 0,
        ..m
    });
    history.push(reading);
    let harness = open(app(history, None));
    harness.get_by_label(text::NO_SWAP);
    assert!(harness.query_by_label(text::MISSING).is_none());
}

#[test]
fn d28_no_gpu_panel_without_an_nvidia_gpu() {
    let mut history = History::default();
    history.push(bare(Duration::from_secs(1), Duration::from_millis(500)));
    let harness = open(app(history, None));
    assert!(
        harness
            .query_by_label_contains(&text::fill(text::GPU_UNREAD, &[("address", "")]))
            .is_none()
    );
    assert!(harness.query_by_label(text::GPU_NO_DRIVER).is_none());
}

#[test]
fn gpu_power_tooltip_names_its_averaging_window_or_says_it_is_instantaneous() {
    let (_, history) = running();
    let power = history::gpu_power_of(history.newest().unwrap(), GPU_ID)
        .unwrap()
        .unwrap();
    let mut harness = open(app(history, None));
    let shown = out_of(text::WATTS, number(power.watts, 0), number(185.0, 0));
    harness.get_by_label(&shown).hover();
    harness.run_steps(4);
    harness.get_by_label(&fill(text::AVERAGED_OVER, &[("seconds", "2.0")]));

    let mut machine = Machine::new(Duration::from_millis(500));
    machine.gpu.power = PowerReading::Instant(Ok(0.0));
    let mut history = History::default();
    machine.run(&mut history, 3);
    let power = history::gpu_power_of(history.newest().unwrap(), GPU_ID)
        .unwrap()
        .unwrap();
    let mut harness = open(app(history, None));
    harness
        .get_by_label(&out_of(
            text::WATTS,
            number(power.watts, 0),
            number(185.0, 0),
        ))
        .hover();
    harness.run_steps(4);
    harness.get_by_label(text::INSTANTANEOUS);
}

#[test]
fn d14_refresh_and_history_survive_a_restart() {
    let mut storage = MemoryStorage::default();
    let (_, history) = running();
    let interval = Arc::new(AtomicU64::new(0));
    let first = App::new(
        Some(&storage),
        Arc::new(Mutex::new(history)),
        interval.clone(),
    );
    assert_eq!(interval.load(Ordering::Relaxed), DEFAULT_REFRESH);
    let mut harness = open(first);
    choose(&mut harness, text::REFRESH, &refresh_text(2000));
    choose(&mut harness, text::HISTORY, &minutes_text(30));
    assert_eq!(interval.load(Ordering::Relaxed), 2000);
    harness.state().settings.save(&mut storage);

    let interval = Arc::new(AtomicU64::new(0));
    let second = App::new(
        Some(&storage),
        Arc::new(Mutex::new(History::default())),
        interval.clone(),
    );
    assert_eq!(interval.load(Ordering::Relaxed), 2000);
    let harness = open(second);
    harness.get_by_label(&summary(2000, 30));
}

/// The top line's interval and span.
fn summary(ms: u64, minutes: u64) -> String {
    fill(
        text::SUMMARY,
        &[
            ("refresh", &refresh_text(ms)),
            ("span", &minutes_text(minutes)),
        ],
    )
}

/// Picks `choice` among the choices of the setting `label`.
fn choose(harness: &mut Harness<'_, App>, label: &str, choice: &str) {
    open_settings(harness);
    // `label` names the setting; its choices are each named by their text.
    let _ = label;
    harness.get_by_label(choice).click_accesskit();
    harness.run_steps(4);
}

fn uncheck(harness: &mut Harness<'_, App>, name: &str, nth: usize) {
    open_settings(harness);
    harness
        .get_all_by_role_and_label(egui::accesskit::Role::CheckBox, name)
        .nth(nth)
        .unwrap()
        .click_accesskit();
    harness.run_steps(4);
    harness.key_press(egui::Key::Escape);
    harness.run_steps(4);
}

#[test]
fn d26_unchecking_the_fan_line_hides_it_and_survives_a_restart() {
    let (_, history) = running();
    let fan = text::FANS;
    let mut harness = open(app(history, None));
    harness.get_by_label(fan);
    uncheck(&mut harness, text::FANS, 0);
    assert!(harness.query_by_label(fan).is_none());
    // The other lines stay.
    harness.get_by_label(text::CLOCK);

    let mut storage = MemoryStorage::default();
    harness.state().settings.save(&mut storage);
    let (_, history) = running();
    let harness = open(app(history, Some(&storage)));
    assert!(harness.query_by_label(fan).is_none());
    harness.get_by_label(text::CLOCK);
}

#[test]
fn d32_only_unchecked_boxes_are_stored_under_their_device_and_kept_while_it_is_absent() {
    let (_, history) = running();
    let mut harness = open(app(history, None));
    let mut storage = MemoryStorage::default();
    harness.state().settings.save(&mut storage);
    assert_eq!(storage.get(HIDDEN_KEY), "");
    uncheck(&mut harness, text::FANS, 0);
    // The temperature box of the GPU comes before the CPU's.
    uncheck(&mut harness, text::TEMPERATURE, 1);
    uncheck(&mut harness, text::DISK_RATES, 0);
    harness.state().settings.save(&mut storage);
    assert_eq!(
        storage.get(HIDDEN_KEY),
        format!("cpu/temperature\ndisk:/dev/nvme0n1p1/rates\ngpu:{GPU_ID}/fans")
    );

    // A restart with neither the GPU nor the disk present keeps both.
    let mut history = History::default();
    history.push(bare(Duration::from_secs(1), Duration::from_millis(500)));
    let restarted = open(app(history, Some(&storage)));
    let mut again = MemoryStorage::default();
    restarted.state().settings.save(&mut again);
    assert_eq!(again.get(HIDDEN_KEY), storage.get(HIDDEN_KEY));
}

impl MemoryStorage {
    fn get(&self, key: &str) -> String {
        self.0.get(key).cloned().unwrap_or_default()
    }
}

#[test]
fn d30_until_set_the_threshold_follows_the_limit_and_once_set_it_stays() {
    let (mut machine, history) = running();
    let mut harness = open(app(history, None));
    open_settings(&mut harness);
    assert_eq!(
        threshold_field(&harness).accesskit_node().numeric_value(),
        Some(176.0)
    );
    machine.gpu.power_limit = Ok(200.0);
    with_history(&mut harness, |h| machine.run(h, 1));
    assert_eq!(
        threshold_field(&harness).accesskit_node().numeric_value(),
        Some(190.0)
    );
    assert!(harness.state().settings.thresholds.is_empty());

    threshold_field(&harness).set_value_accesskit(150.0);
    harness.run_steps(4);
    assert_eq!(
        harness.state().settings.thresholds.get(GPU_ID),
        Some(&150.0)
    );
    machine.gpu.power_limit = Ok(185.0);
    with_history(&mut harness, |h| machine.run(h, 1));
    assert_eq!(
        threshold_field(&harness).accesskit_node().numeric_value(),
        Some(150.0)
    );
}

#[test]
fn d30_a_threshold_typed_in_the_field_is_set() {
    let (_, history) = running();
    let mut harness = open(app(history, None));
    open_settings(&mut harness);
    threshold_field(&harness).click();
    harness.run_steps(4);
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    threshold_field(&harness).type_text("160");
    harness.key_press(egui::Key::Enter);
    harness.run_steps(4);
    assert_eq!(
        harness.state().settings.thresholds.get(GPU_ID),
        Some(&160.0)
    );
}

#[test]
fn d30_a_value_egui_changes_by_itself_never_sets_the_threshold() {
    // No limit: the line asks for a threshold, through several frames.
    let mut machine = Machine::new(Duration::from_millis(500));
    machine.gpu.power_limit = Err(Missing::NotReported);
    machine.gpu.power_limit_range = Err(Missing::NotReported);
    let mut history = History::default();
    machine.run(&mut history, 10);
    let mut harness = open(app(history, None));
    harness.run_steps(10);
    harness.get_by_label(text::SET_THRESHOLD);
    open_settings(&mut harness);
    // The field shows the mark of a missing value, not a stand-in number.
    assert_eq!(
        threshold_field(&harness)
            .accesskit_node()
            .value()
            .as_deref(),
        Some(text::MISSING)
    );
    assert!(harness.state().settings.thresholds.is_empty());
    // Typed into, the empty field sets the threshold, and then shows it.
    threshold_field(&harness).click();
    harness.run_steps(4);
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    threshold_field(&harness).type_text("150");
    harness.key_press(egui::Key::Enter);
    harness.run_steps(4);
    assert_eq!(
        harness.state().settings.thresholds.get(GPU_ID),
        Some(&150.0)
    );
    assert_eq!(
        threshold_field(&harness)
            .accesskit_node()
            .value()
            .as_deref(),
        Some(watts(150.0).as_str())
    );

    // A default above the highest limit the card allows is clamped by egui.
    let mut machine = Machine::new(Duration::from_millis(500));
    machine.gpu.power_limit_range = Ok((50.0, 150.0));
    let mut history = History::default();
    machine.run(&mut history, 10);
    let mut harness = open(app(history, None));
    harness.run_steps(10);
    assert!(harness.state().settings.thresholds.is_empty());
}

#[test]
fn d31_each_gpu_keeps_its_own_threshold_which_survives_a_restart_and_its_absence() {
    let (mut machine, mut history) = running();
    let second = GpuReading {
        id: "GPU-second".into(),
        name: "Second".into(),
        ..machine.gpu.clone()
    };
    let mut reading = machine.reading();
    let GpuState::Running(gpus) = &mut reading.gpu else {
        unreachable!()
    };
    gpus.push(second);
    for _ in 0..5 {
        reading.time += Duration::from_millis(500);
        history.push(reading.clone());
    }
    let mut harness = open(app(history, None));
    open_settings(&mut harness);
    threshold_of_gpu(&harness, "Second").set_value_accesskit(120.0);
    harness.run_steps(4);
    let thresholds = &harness.state().settings.thresholds;
    assert_eq!(thresholds.len(), 1, "the unset GPU stores nothing");
    assert_eq!(thresholds.get("GPU-second"), Some(&120.0));

    let mut storage = MemoryStorage::default();
    harness.state().settings.save(&mut storage);
    // A restart with only the first GPU, then a save, keeps the second's.
    let (_, history) = running();
    let mut restarted = open(app(history, Some(&storage)));
    open_settings(&mut restarted);
    assert_eq!(
        threshold_field(&restarted).accesskit_node().numeric_value(),
        Some(176.0)
    );
    let mut again = MemoryStorage::default();
    restarted.state().settings.save(&mut again);
    assert_eq!(again.get(THRESHOLDS_KEY), "GPU-second 120");
}

/// A state of the window: its name, the window, what the harness does before
/// the photograph, and what shows that the window is in that state.
type Scenario = (
    &'static str,
    App,
    fn(&mut Harness<'_, App>),
    fn(&Harness<'_, App>),
);

fn scenarios() -> Vec<Scenario> {
    fn nothing(_: &mut Harness<'_, App>) {}
    fn live(h: &Harness<'_, App>) {
        assert!(h.query_by_label(text::MISSING).is_none());
        h.get_by_label(text::OVER_THRESHOLD);
    }
    fn settings_open(h: &Harness<'_, App>) {
        h.get_by_label(&refresh_text(5000));
        h.get_by_label(&minutes_text(60));
        h.get_by_label(text::GRAPH_GPU);
    }
    fn first(h: &Harness<'_, App>) {
        h.get_by_label(text::FIRST_READING);
    }
    fn waiting(h: &Harness<'_, App>) {
        h.get_all_by_label(text::MISSING).next().unwrap();
    }
    fn lost(h: &Harness<'_, App>) {
        h.get_by_label_contains(text::GPU_LOST);
    }
    fn no_limit(h: &Harness<'_, App>) {
        h.get_by_label(text::SET_THRESHOLD);
    }
    fn suspended(h: &Harness<'_, App>) {
        let history = h.state().history.lock().unwrap();
        assert!(!history.stretches(Duration::from_secs(300)).is_empty());
    }
    fn two_gpus(h: &Harness<'_, App>) {
        h.get_by_label(&fill(
            text::NUMBERED_LINE,
            &[("name", text::UTILIZATION), ("n", "2")],
        ));
        h.get_by_label(text::NO_SWAP);
    }
    fn failed(h: &Harness<'_, App>) {
        assert_eq!(h.get_all_by_label_contains(text::READ_FAILED).count(), 2);
        h.get_by_label(text::NO_DISKS);
        h.get_by_label(text::NO_NETWORK);
    }
    fn open_menu(harness: &mut Harness<'_, App>) {
        open_settings(harness);
    }
    fn not_started(failure: GpuFailure) -> App {
        let mut history = History::default();
        let mut machine = Machine::new(Duration::from_millis(500));
        for _ in 0..10 {
            let mut reading = machine.reading();
            reading.gpu = GpuState::NotStarted {
                failure,
                cards: vec!["0000:07:00.0".into()],
            };
            history.push(reading);
        }
        app(history, None)
    }

    let mut scenarios: Vec<Scenario> = vec![
        ("running", app(running().1, None), nothing, live),
        ("settings", app(running().1, None), open_menu, settings_open),
        (
            "first-reading",
            app(History::default(), None),
            nothing,
            first,
        ),
    ];

    let mut one = History::default();
    Machine::new(Duration::from_millis(500)).run(&mut one, 1);
    scenarios.push(("one-reading", app(one, None), nothing, waiting));

    fn no_driver(h: &Harness<'_, App>) {
        h.get_by_label(text::GPU_NO_DRIVER);
    }
    fn not_running(h: &Harness<'_, App>) {
        h.get_by_label(text::GPU_DRIVER_NOT_RUNNING);
    }
    fn mismatch(h: &Harness<'_, App>) {
        h.get_by_label(text::GPU_VERSION_MISMATCH);
    }
    fn no_permission(h: &Harness<'_, App>) {
        h.get_by_label(text::GPU_NO_PERMISSION);
    }
    fn other(h: &Harness<'_, App>) {
        h.get_by_label(text::GPU_OTHER_FAILURE);
    }
    type Check = fn(&Harness<'_, App>);
    for (name, failure, check) in [
        ("gpu-no-driver", GpuFailure::NoLibrary, no_driver as Check),
        (
            "gpu-driver-not-running",
            GpuFailure::DriverNotLoaded,
            not_running,
        ),
        (
            "gpu-version-mismatch",
            GpuFailure::VersionMismatch,
            mismatch,
        ),
        ("gpu-no-permission", GpuFailure::NoPermission, no_permission),
        ("gpu-other-failure", GpuFailure::Other, other),
    ] {
        scenarios.push((name, not_started(failure), nothing, check));
    }

    let (mut machine, mut history) = running();
    machine.gpu = GpuReading {
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
        ..machine.gpu.clone()
    };
    machine.run(&mut history, 20);
    scenarios.push(("gpu-lost", app(history, None), nothing, lost));

    let mut machine = Machine::new(Duration::from_millis(500));
    machine.gpu.power_limit = Err(Missing::NotReported);
    machine.gpu.power_limit_range = Err(Missing::NotReported);
    machine.gpu.fans = Err(Missing::NotReported);
    machine.gpu.power = PowerReading::Instant(Ok(0.0));
    let mut history = History::default();
    machine.run(&mut history, 600);
    scenarios.push(("gpu-no-limit-no-fan", app(history, None), nothing, no_limit));

    let (mut machine, mut history) = running();
    machine.time += Duration::from_secs(90);
    machine.run(&mut history, 100);
    scenarios.push(("suspend", app(history, None), nothing, suspended));

    let (mut machine, mut history) = running();
    let mut reading = machine.reading();
    let GpuState::Running(gpus) = &mut reading.gpu else {
        unreachable!()
    };
    let second = GpuReading {
        id: "GPU-second".into(),
        ..gpus[0].clone()
    };
    gpus.push(second);
    reading.memory = reading.memory.map(|m| Memory {
        swap_used: 0,
        swap_total: 0,
        ..m
    });
    history.push(reading);
    scenarios.push((
        "two-gpus-of-one-model-no-swap",
        app(history, None),
        nothing,
        two_gpus,
    ));

    let (mut machine, mut history) = running();
    let mut reading = machine.reading();
    reading.cpu = Err(Missing::Failed);
    reading.memory = Err(Missing::Failed);
    reading.cpu_frequency = Err(Missing::Failed);
    reading.cpu_temperature = Err(Missing::NotReported);
    reading.disks.clear();
    reading.networks.clear();
    history.push(reading);
    scenarios.push((
        "read-failed-no-disks-no-network",
        app(history, None),
        nothing,
        failed,
    ));

    scenarios
}

#[test]
fn every_scenario_shows_the_state_it_is_named_for() {
    for (name, app, prepare, check) in scenarios() {
        let mut harness = open(app);
        prepare(&mut harness);
        harness.run_steps(4);
        // Shown only on a failure, naming the scenario that failed.
        eprintln!("{name}");
        check(&harness);
    }
}

/// Writes one image per scenario into `target/captures/`, for the copy step
/// (D19). It needs a graphics adapter, which the CI runners do not have, so
/// it runs by hand: `cargo test -- --ignored capture_scenarios`.
#[test]
#[ignore]
fn capture_scenarios() {
    let folder = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/captures");
    std::fs::create_dir_all(&folder).unwrap();
    for (name, app, prepare, _) in scenarios() {
        // The settings' menu scrolls, so it is also photographed whole.
        let heights: &[(f32, &str)] = if name == "settings" {
            &[(600.0, ""), (1300.0, "-whole")]
        } else {
            &[(600.0, "")]
        };
        let mut app = Some(app);
        for &(height, suffix) in heights {
            let app = app
                .take()
                .unwrap_or_else(|| super::tests::app(running().1, None));
            let mut harness = open_sized(app, WINDOW_SIZE[0], height);
            prepare(&mut harness);
            harness.run_steps(4);
            let image = harness
                .render()
                .expect("a graphics adapter renders the window");
            image
                .save(folder.join(format!("{name}{suffix}.png")))
                .unwrap();
            // Every text the image shows, for the index of its slots.
            let texts: Vec<String> = (harness.query_all(By::new().predicate(|_| true)))
                .flat_map(|node| {
                    let node = node.accesskit_node();
                    [node.label(), node.value()]
                })
                .flatten()
                .collect();
            std::fs::write(
                folder.join(format!("{name}{suffix}.texts")),
                texts.join("\n"),
            )
            .unwrap();
        }
    }
}

#[test]
fn a_window_whose_measurements_stop_says_so() {
    let (_, history) = running();
    let newest = history.newest().unwrap().reading.time;
    let mut harness = open(app(history, None));
    assert!(harness.query_by_label_contains(&stopped_prefix()).is_none());
    // Three intervals of 500 ms and a second late is still live; later is not.
    harness.state_mut().clock = Box::new(move || newest + Duration::from_millis(2400));
    harness.run_steps(2);
    assert!(harness.query_by_label_contains(&stopped_prefix()).is_none());
    harness.state_mut().clock = Box::new(move || newest + Duration::from_secs(65));
    harness.run_steps(2);
    harness.get_by_label(&text::fill(text::STOPPED, &[("time", "1:05")]));
}

/// The part of the sentence for stopped measurements before its time.
fn stopped_prefix() -> String {
    text::STOPPED.split("{time}").next().unwrap().to_string()
}

#[test]
fn d48_a_disks_pie_turns_yellow_at_70_and_red_at_90() {
    assert_eq!(space_color(0.0), SPACE_GREEN);
    assert_eq!(space_color(69.9), SPACE_GREEN);
    assert_eq!(space_color(70.0), SPACE_YELLOW);
    assert_eq!(space_color(89.9), SPACE_YELLOW);
    assert_eq!(space_color(90.0), SPACE_RED);
    assert_eq!(space_color(100.0), SPACE_RED);
}

#[test]
fn d48_each_disk_shows_how_full_it_is_beside_its_name() {
    let (_, history) = running();
    let harness = open(app(history, None));
    // 419.5 of 480.0 GB used: 87%.
    let pie = (harness.query_all(By::new().role(egui::accesskit::Role::Image)))
        .find(|node| {
            node.accesskit_node()
                .label()
                .is_some_and(|l| l.contains("GiB"))
        })
        .unwrap();
    let label = pie.accesskit_node().label().unwrap_or_default();
    assert!(label.contains(&percent(87.4)), "{label}");
    let name = harness.get_by_label("nvme0n1p1").rect();
    assert!((pie.rect().center().y - name.center().y).abs() < 2.0);
    // The texts below the grid no longer give the disk's space.
    assert_eq!(harness.get_all_by_label("nvme0n1p1").count(), 1);
}

/// The window's height after a few frames: what it asks the system for.
fn wanted_height(harness: &Harness<'_, App>) -> f32 {
    harness.state().wanted_height
}

#[test]
fn d49_the_window_is_as_tall_as_what_it_shows() {
    let (_, history) = running();
    let harness = open(app(history, None));
    let one = wanted_height(&harness);
    let lowest = (harness.query_all(By::new().predicate(|node| node.bounding_box().is_some())))
        .map(|node| node.rect().bottom())
        .fold(0.0, f32::max);
    assert!(
        (one - lowest).abs() <= 8.0,
        "asks for {one}, shows down to {lowest}"
    );
    // Two more disks make it taller by about two lines.
    let (mut machine, mut history) = running();
    for _ in 0..2 {
        let mut reading = machine.reading();
        for name in ["/dev/sda1", "/dev/sdb1"] {
            let mut disk = reading.disks[0].clone();
            disk.device = name.into();
            reading.disks.push(disk);
        }
        history.push(reading);
    }
    let harness = open(app(history, None));
    let three = wanted_height(&harness);
    assert!((25.0..40.0).contains(&(three - one)), "{one} then {three}");
}

#[test]
fn d43_the_first_window_shows_every_line_without_scrolling() {
    let [width, height] = WINDOW_SIZE;
    let (_, history) = running();
    let harness = open_sized(app(history, None), width, height);
    let nodes = harness.query_all(By::new().predicate(|node| node.bounding_box().is_some()));
    for node in nodes {
        let rect = node.rect();
        assert!(
            rect.bottom() <= height,
            "{:?} ends at {}",
            node.accesskit_node().label(),
            rect.bottom()
        );
        assert!(
            rect.right() <= width,
            "{:?} ends at {}",
            node.accesskit_node().label(),
            rect.right()
        );
    }
}

#[test]
fn d44_a_value_sits_right_after_the_names() {
    let (_, history) = running();
    let harness = open(app(history, None));
    // The widest name is the disk's, with its pie (D48); every value starts
    // just after it.
    let disk = harness.get_all_by_label("nvme0n1p1").next().unwrap().rect();
    let names = harness
        .query_all(By::new().role(egui::accesskit::Role::Image))
        .map(|node| node.rect())
        .find(|pie| (pie.center().y - disk.center().y).abs() < 2.0 && pie.left() > disk.left())
        .unwrap();
    let ram = harness.get_by_label(text::MEMORY).rect();
    let value = harness
        .get_all_by_label_contains(text::GIB.split(' ').nth(1).unwrap())
        .find(|node| (node.rect().center().y - ram.center().y).abs() < 2.0)
        .unwrap()
        .rect();
    assert!(
        value.left() >= names.right(),
        "the value overlaps the names"
    );
    assert!(
        value.left() - names.right() <= 8.0,
        "a gap of {}",
        value.left() - names.right()
    );
}

#[test]
fn d45_the_names_say_neither_busy_nor_used() {
    let (_, history) = running();
    let mut harness = open(app(history, None));
    for name in [
        text::UTILIZATION,
        text::VRAM,
        text::USAGE,
        text::MEMORY,
        text::SWAP,
    ] {
        harness.get_by_label(name);
    }
    open_settings(&mut harness);
    for word in ["busy", "Busy", "used", "Used"] {
        let found: Vec<_> = harness
            .query_all_by_label_contains(word)
            .map(|node| node.accesskit_node().label())
            .collect();
        assert!(found.is_empty(), "{word}: {found:?}");
    }
}

/// Renders the running window at three sizes, into
/// `target/captures/sizes/`, to check the layout by eye.
#[test]
#[ignore = "needs a graphics adapter"]
fn capture_sizes() {
    let folder = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/captures/sizes");
    std::fs::create_dir_all(&folder).unwrap();
    for (width, height) in [
        (WINDOW_SIZE[0], WINDOW_SIZE[1]),
        (450.0, 300.0),
        (1000.0, 760.0),
    ] {
        let (_, history) = running();
        let mut harness = open_sized(app(history, None), width, height);
        harness.run_steps(4);
        let image = harness
            .render()
            .expect("a graphics adapter renders the window");
        image
            .save(folder.join(format!("{width}x{height}.png")))
            .unwrap();
    }
}
