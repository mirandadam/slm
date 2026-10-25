//! The window (D43, D44): a top line with the settings behind one button,
//! then one line per measurement, with its name, its value and a small chart
//! of its recent history, then the rest as text. The window only draws what
//! the history holds, which the sampling thread fills.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use eframe::egui::{self, Color32, Pos2, Rect, Response, RichText, Sense, Shape, Stroke, Ui};

use crate::history::{self, Entry, GpuPower, History};
use crate::reading::{GpuFailure, GpuReading, GpuState, Missing, Value};
use crate::sampler;
use crate::text::{self, fill};

/// The refresh intervals offered, in milliseconds. 200 ms is the lowest,
/// because `/proc/stat` counts in ticks of 10 ms, so a core's usage over
/// 200 ms already moves in steps of 5%.
pub const REFRESH_CHOICES: [u64; 5] = [200, 500, 1000, 2000, 5000];
pub const DEFAULT_REFRESH: u64 = 500;
/// The history lengths offered, in minutes. 60 is the most, because the
/// history keeps one hour (D16).
const HISTORY_CHOICES: [u64; 5] = [1, 5, 15, 30, 60];
const DEFAULT_HISTORY: u64 = 5;

/// The window's size before it measures what it shows: a small window
/// beside the main work (D43). Its width stays; its height then follows what
/// it shows (D49).
pub const WINDOW_SIZE: [f32; 2] = [350.0, 200.0];

/// The keys of the settings in eframe's storage (D14, D31, D32).
const REFRESH_KEY: &str = "refresh_ms";
const HISTORY_KEY: &str = "history_minutes";
const HIDDEN_KEY: &str = "hidden";
const THRESHOLDS_KEY: &str = "thresholds";

/// The WCAG 2.2 minimum for a target under a pointer, which the settings
/// button and the controls of its menu keep.
const TARGET_HEIGHT: f32 = 24.0;
/// The size of the window's text.
const FONT_SIZE: f32 = 11.0;
/// The height of a line of the grid and of its chart.
const LINE_HEIGHT: f32 = 13.0;
/// The widest the bars of the logical CPUs get, together.
const CORES_WIDTH: f32 = 60.0;

const BLUE: Color32 = Color32::from_rgb(0x3b, 0x82, 0xd6);
const ORANGE: Color32 = Color32::from_rgb(0xe0, 0x84, 0x2a);
const GREY: Color32 = Color32::from_rgb(0x88, 0x88, 0x88);
const GREEN: Color32 = Color32::from_rgb(0x4c, 0xaf, 0x50);
const SPACE_GREEN: Color32 = Color32::from_rgb(0x4c, 0xaf, 0x50);
const SPACE_YELLOW: Color32 = Color32::from_rgb(0xe8, 0xc5, 0x47);
const SPACE_RED: Color32 = Color32::from_rgb(0xd6, 0x45, 0x45);

/// The color of a disk's pie, by the share of its space used (D48).
fn space_color(share: f64) -> Color32 {
    if share >= 90.0 {
        SPACE_RED
    } else if share >= 70.0 {
        SPACE_YELLOW
    } else {
        SPACE_GREEN
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub refresh_ms: u64,
    pub history_minutes: u64,
    /// The keys of the boxes the person unchecked in the menu of what is
    /// shown, kept while their device is absent (D32).
    pub hidden: BTreeSet<String>,
    /// The thresholds the person set, in W, under each GPU's UUID, kept while
    /// the GPU is absent (D31). A GPU never set has none, so that its default
    /// keeps following its limit.
    pub thresholds: BTreeMap<String, f64>,
}

impl Settings {
    pub fn load(storage: Option<&dyn eframe::Storage>) -> Self {
        let get = |key| storage.and_then(|s| s.get_string(key));
        let number = |key, choices: &[u64], default| {
            get(key)
                .and_then(|v| v.parse().ok())
                .filter(|v| choices.contains(v))
                .unwrap_or(default)
        };
        Self {
            refresh_ms: number(REFRESH_KEY, &REFRESH_CHOICES, DEFAULT_REFRESH),
            history_minutes: number(HISTORY_KEY, &HISTORY_CHOICES, DEFAULT_HISTORY),
            hidden: get(HIDDEN_KEY)
                .map(|v| v.lines().map(str::to_string).collect())
                .unwrap_or_default(),
            thresholds: get(THRESHOLDS_KEY)
                .map(|v| {
                    v.lines()
                        .filter_map(|line| {
                            let (id, watts) = line.rsplit_once(' ')?;
                            Some((id.to_string(), watts.parse().ok()?))
                        })
                        .collect()
                })
                .unwrap_or_default(),
        }
    }

    pub fn save(&self, storage: &mut dyn eframe::Storage) {
        storage.set_string(REFRESH_KEY, self.refresh_ms.to_string());
        storage.set_string(HISTORY_KEY, self.history_minutes.to_string());
        let hidden: Vec<&str> = self.hidden.iter().map(String::as_str).collect();
        storage.set_string(HIDDEN_KEY, hidden.join("\n"));
        let thresholds: Vec<String> = (self.thresholds.iter())
            .map(|(id, watts)| format!("{id} {watts}"))
            .collect();
        storage.set_string(THRESHOLDS_KEY, thresholds.join("\n"));
    }

    fn span(&self) -> Duration {
        Duration::from_secs(self.history_minutes * 60)
    }

    fn shown(&self, key: &str) -> bool {
        !self.hidden.contains(key)
    }
}

pub struct App {
    history: Arc<Mutex<History>>,
    /// The refresh interval the sampling thread reads, in milliseconds.
    interval: Arc<AtomicU64>,
    pub settings: Settings,
    styled: bool,
    /// The sampling thread's clock, and its time when the window opened, to
    /// tell when measuring has stopped.
    clock: Box<dyn Fn() -> Duration>,
    opened: Duration,
    pub wanted_height: f32,
}

impl App {
    pub fn new(
        storage: Option<&dyn eframe::Storage>,
        history: Arc<Mutex<History>>,
        interval: Arc<AtomicU64>,
    ) -> Self {
        let settings = Settings::load(storage);
        interval.store(settings.refresh_ms, Ordering::Relaxed);
        Self {
            history,
            interval,
            settings,
            styled: false,
            clock: Box::new(sampler::boottime),
            opened: sampler::boottime(),
            wanted_height: 0.0,
        }
    }

    pub fn show(&mut self, ui: &mut Ui) {
        if !self.styled {
            ui.ctx().all_styles_mut(|style| {
                style.spacing.interact_size.y = TARGET_HEIGHT;
                // The size of the mock-up the requester chose (D44), so that
                // every line fits the small window.
                for text_style in [egui::TextStyle::Body, egui::TextStyle::Button] {
                    if let Some(font) = style.text_styles.get_mut(&text_style) {
                        font.size = FONT_SIZE;
                    }
                }
            });
            self.styled = true;
        }
        let history = self.history.clone();
        let history = history.lock().unwrap();
        let stopped = self.stopped_for(&history);
        let view = View {
            history: &history,
            newest: history.newest(),
            span: self.settings.span(),
        };
        let margin = egui::Margin::symmetric(4, 2);
        let top = egui::Panel::top("settings")
            .frame(egui::Frame::side_top_panel(ui.style()).inner_margin(margin))
            .show(ui, |ui| {
                self.top_line(ui, &view);
                if let Some(time) = stopped {
                    warning(
                        ui,
                        &fill(text::STOPPED, &[("time", &minutes_seconds(time))]),
                    );
                }
            });
        // Without new readings nothing else repaints the window, and the
        // sentence above must still appear.
        ui.ctx().request_repaint_after(Duration::from_secs(1));
        let content = egui::CentralPanel::default()
            .frame(egui::Frame::central_panel(ui.style()).inner_margin(margin))
            .show(ui, |ui| {
                // It scrolls only when the screen is shorter than what it shows.
                egui::ScrollArea::vertical()
                    .show(ui, |ui| {
                        // Nothing here is pressed, so the lines need not be as
                        // tall as a target.
                        ui.spacing_mut().interact_size.y = LINE_HEIGHT;
                        ui.spacing_mut().item_spacing.y = 1.0;
                        measurements(ui, &view, &self.settings);
                    })
                    .content_size
            })
            .inner;
        // The window is as tall as what it shows (D49).
        let height = (top.response.rect.height() + content.y + 2.0 * margin.topf()).ceil();
        if (height - self.wanted_height).abs() >= 1.0 {
            self.wanted_height = height;
            let size = egui::vec2(WINDOW_SIZE[0], height);
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::InnerSize(size));
        }
    }

    /// How long no reading has come, once that is more than three intervals
    /// and a second, which no slow read reaches: the sampling thread has
    /// stopped or hangs, and the window would otherwise look live.
    fn stopped_for(&self, history: &History) -> Option<Duration> {
        let (since, interval) = match history.newest() {
            Some(entry) => (entry.reading.time, entry.reading.interval),
            None => (self.opened, Duration::from_millis(self.settings.refresh_ms)),
        };
        let age = (self.clock)().saturating_sub(since);
        (age > interval * 3 + Duration::from_secs(1)).then_some(age)
    }

    /// The GPU's name when there is one, the interval and the span in short,
    /// and the button of the settings.
    fn top_line(&mut self, ui: &mut Ui, view: &View) {
        ui.horizontal(|ui| {
            if let [gpu] = running_gpus(view) {
                about(ui.label(RichText::new(&gpu.name).strong()), gpu_about(gpu));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // The menu stays open until a click outside it, so that the
                // person can change several things in one visit.
                let config = egui::containers::menu::MenuConfig::new()
                    .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside);
                egui::containers::menu::MenuButton::new(text::SETTINGS)
                    .config(config)
                    .ui(ui, |ui| {
                        let height = ui.ctx().content_rect().height() - TARGET_HEIGHT;
                        egui::ScrollArea::vertical()
                            .max_height(height)
                            .show(ui, |ui| self.settings_menu(ui, view));
                    });
                ui.label(fill(
                    text::SUMMARY,
                    &[
                        ("refresh", &refresh_text(self.settings.refresh_ms)),
                        ("span", &minutes_text(self.settings.history_minutes)),
                    ],
                ));
            });
        });
    }

    /// "Measure every", "Charts show last", each GPU's threshold, and the
    /// boxes of what is shown (D26).
    fn settings_menu(&mut self, ui: &mut Ui, view: &View) {
        let refresh = &mut self.settings.refresh_ms;
        choices(ui, text::REFRESH, refresh, &REFRESH_CHOICES, refresh_text);
        self.interval.store(*refresh, Ordering::Relaxed);
        let minutes = &mut self.settings.history_minutes;
        choices(ui, text::HISTORY, minutes, &HISTORY_CHOICES, minutes_text);
        for (gpu, title) in running_gpus(view)
            .iter()
            .zip(gpu_titles(running_gpus(view)))
        {
            threshold_field(ui, view, gpu, &title, &mut self.settings);
        }
        ui.separator();
        ui.label(RichText::new(text::SHOWN).strong());
        shown_menu(ui, view.newest, &mut self.settings);
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        self.show(ui);
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        self.settings.save(storage);
    }
}

/// A setting's name and its choices, of which one is selected.
fn choices(ui: &mut Ui, name: &str, selected: &mut u64, choices: &[u64], text: fn(u64) -> String) {
    ui.label(name);
    ui.horizontal_wrapped(|ui| {
        for &choice in choices {
            ui.selectable_value(selected, choice, text(choice));
        }
    });
}

fn refresh_text(ms: u64) -> String {
    if ms < 1000 {
        fill(text::MILLISECONDS, &[("n", &ms.to_string())])
    } else {
        fill(text::SECONDS, &[("n", &(ms / 1000).to_string())])
    }
}

fn minutes_text(minutes: u64) -> String {
    fill(text::MINUTES, &[("n", &minutes.to_string())])
}

/// What every line draws from.
struct View<'a> {
    history: &'a History,
    newest: Option<&'a Entry>,
    span: Duration,
}

fn running_gpus<'a>(view: &View<'a>) -> &'a [GpuReading] {
    match view.newest.map(|e| &e.reading.gpu) {
        Some(GpuState::Running(gpus)) => gpus,
        _ => &[],
    }
}

// The lines, charts and texts of each device, by the key they are stored
// under in the menu of what is shown (D32), and the name the menu gives them.
const GPU_ITEMS: [(&str, &str); 10] = [
    ("utilization", text::UTILIZATION),
    ("utilization-chart", text::GRAPH_GPU),
    ("vram", text::VRAM),
    ("vram-chart", text::GRAPH_VRAM),
    ("power", text::POWER),
    ("power-chart", text::GRAPH_POWER),
    ("over-threshold", text::OVER_THRESHOLD),
    ("temperature", text::TEMPERATURE),
    ("fans", text::FANS),
    ("clock", text::CLOCK),
];
const CPU_ITEMS: [(&str, &str); 5] = [
    ("usage", text::USAGE),
    ("usage-chart", text::GRAPH_CPU),
    ("cores", text::CORES),
    ("temperature", text::TEMPERATURE),
    ("frequency", text::FREQUENCY),
];
const MEMORY_ITEMS: [(&str, &str); 3] = [
    ("memory", text::MEMORY),
    ("memory-chart", text::GRAPH_MEMORY),
    ("swap", text::SWAP),
];
const DISK_ITEMS: [(&str, &str); 3] = [
    ("rates", text::DISK_RATES),
    ("rates-chart", text::GRAPH_DISK),
    ("space", text::SPACE),
];
const NETWORK_ITEMS: [(&str, &str); 2] = [
    ("rates", text::NETWORK_RATES),
    ("rates-chart", text::GRAPH_NETWORK),
];

fn key(device: &str, item: &str) -> String {
    format!("{device}/{item}")
}

fn gpu_key(gpu: &GpuReading) -> String {
    format!("gpu:{}", gpu.id)
}

fn disk_key(device: &str) -> String {
    format!("disk:{device}")
}

fn network_key(interface: &str) -> String {
    format!("network:{interface}")
}

fn disk_title(disk: &crate::reading::DiskReading) -> String {
    let mounts: Vec<String> = (disk.mount_points.iter())
        .map(|p| p.display().to_string())
        .collect();
    fill(
        text::DISK,
        &[("device", &disk.device), ("mounts", &mounts.join(", "))],
    )
}

/// A disk's title, then what it is (D50).
fn disk_about(disk: &crate::reading::DiskReading) -> String {
    let info = &disk.info;
    let kind = info
        .solid_state
        .map(|ssd| if ssd { text::SSD } else { text::HDD });
    let kind = [kind, info.file_system.as_deref()];
    let kind = kind.into_iter().flatten().collect::<Vec<_>>().join(", ");
    let kind = (!kind.is_empty()).then_some(kind);
    lines([Some(disk_title(disk)), info.model.clone(), kind]).unwrap_or_default()
}

/// What a network adapter is, and its link now (D50).
fn network_about(net: &crate::reading::NetworkReading) -> Option<String> {
    let link = net.link.map(|(speed, full)| {
        fill(
            text::LINK,
            &[
                ("speed", &speed.to_string()),
                ("rate", &rate(speed as f64 * 1e6 / 8.0)),
                (
                    "duplex",
                    if full {
                        text::FULL_DUPLEX
                    } else {
                        text::HALF_DUPLEX
                    },
                ),
            ],
        )
    });
    let driver = (net.info.driver.as_ref()).map(|d| fill(text::DRIVER, &[("driver", d)]));
    lines([link, net.info.model.clone(), driver])
}

/// What the CPU is (D50).
fn cpu_about(info: &crate::reading::CpuInfo) -> Option<String> {
    let cores = info.cores.zip(info.logical).map(|(cores, logical)| {
        fill(
            text::CPU_CORES,
            &[
                ("cores", &cores.to_string()),
                ("logical", &logical.to_string()),
            ],
        )
    });
    lines([info.model.clone(), cores])
}

/// What a GPU is: its architecture and VRAM, its PCIe link, its driver (D50).
fn gpu_about(gpu: &GpuReading) -> Option<String> {
    let info = &gpu.info;
    let vram = (gpu.memory_total.ok()).map(|b| fill(text::GIB, &[("value", &gib_number(b))]));
    let kind = match (&info.architecture, vram) {
        (Some(architecture), Some(vram)) => Some(fill(
            text::GPU_KIND,
            &[("architecture", architecture), ("vram", &vram)],
        )),
        (architecture, vram) => architecture.clone().or(vram),
    };
    let pcie = |(generation, width): (u32, u32)| {
        fill(
            text::PCIE,
            &[
                ("generation", &generation.to_string()),
                ("width", &width.to_string()),
            ],
        )
    };
    let link = match (info.max_link, gpu.link.ok()) {
        (Some(max), Some(now)) => Some(fill(
            text::GPU_LINK,
            &[("max", &pcie(max)), ("now", &pcie(now))],
        )),
        (max, now) => max.or(now).map(pcie),
    };
    let driver = (info.driver.as_ref()).map(|driver| match info.cuda {
        Some((major, minor)) => fill(
            text::GPU_DRIVER,
            &[("driver", driver), ("cuda", &format!("{major}.{minor}"))],
        ),
        None => fill(text::DRIVER, &[("driver", driver)]),
    });
    lines([kind, link, driver])
}

/// The GPU's power limit, its default, and the range it can be set in (D50).
fn power_limits(gpu: &GpuReading) -> Option<String> {
    let (min, max) = gpu.power_limit_range.ok()?;
    Some(fill(
        text::POWER_LIMITS,
        &[
            ("limit", &exact_watts(gpu.power_limit.ok()?)),
            ("default", &exact_watts(gpu.info.default_limit?)),
            ("min", &exact_watts(min)),
            ("max", &exact_watts(max)),
        ],
    ))
}

/// Watts with a decimal only where there is one, as a limit of 92.5 W.
fn exact_watts(value: f64) -> String {
    let decimals = if value.fract() == 0.0 { 0 } else { 1 };
    fill(text::WATTS, &[("value", &number(value, decimals))])
}

/// The lines that are known, one per line of a tooltip, or none (D50).
fn lines(parts: impl IntoIterator<Item = Option<String>>) -> Option<String> {
    let known: Vec<String> = parts.into_iter().flatten().collect();
    (!known.is_empty()).then(|| known.join("\n"))
}

/// Gives a name its tooltip, where there is one.
fn about(response: Response, about: Option<String>) -> Response {
    match about {
        Some(text) => response.on_hover_text(text),
        None => response,
    }
}

/// A disk's short name, the last part of its device, as `nvme0n1p1`.
fn disk_name(device: &str) -> &str {
    device.rsplit('/').next().unwrap_or(device)
}

/// The boxes of what is shown (D26): each device by its name, and under it a
/// box for each line, chart or text it shows.
fn shown_menu(ui: &mut Ui, newest: Option<&Entry>, settings: &mut Settings) {
    let mut boxes = |ui: &mut Ui, device: &str, items: &[(&str, &str)]| {
        for (item, name) in items {
            let key = key(device, item);
            let mut shown = settings.shown(&key);
            if ui.checkbox(&mut shown, *name).changed() {
                if shown {
                    settings.hidden.remove(&key);
                } else {
                    settings.hidden.insert(key);
                }
            }
        }
    };
    if let Some(GpuState::Running(gpus)) = newest.map(|e| &e.reading.gpu) {
        for (gpu, title) in gpus.iter().zip(gpu_titles(gpus)) {
            ui.label(RichText::new(title).strong());
            boxes(ui, &gpu_key(gpu), &GPU_ITEMS);
        }
    }
    ui.label(RichText::new(text::CPU).strong());
    boxes(ui, "cpu", &CPU_ITEMS);
    ui.label(RichText::new(text::MEMORY_PANEL).strong());
    boxes(ui, "memory", &MEMORY_ITEMS);
    if let Some(entry) = newest {
        for disk in &entry.reading.disks {
            ui.label(RichText::new(disk_title(disk)).strong());
            boxes(ui, &disk_key(&disk.device), &DISK_ITEMS);
        }
        for net in &entry.reading.networks {
            ui.label(RichText::new(fill(text::INTERFACE, &[("name", &net.interface)])).strong());
            boxes(ui, &network_key(&net.interface), &NETWORK_ITEMS);
        }
    }
}

/// A sentence that says why something is missing, or less than it should be.
fn warning(ui: &mut Ui, sentence: &str) {
    ui.colored_label(ui.visuals().warn_fg_color, sentence);
}

fn reason(missing: Missing) -> &'static str {
    match missing {
        Missing::NotReported => text::NOT_REPORTED,
        Missing::Waiting => text::WAITING,
        Missing::NoRate => text::NO_RATE,
        Missing::Failed => text::FAILED,
    }
}

/// A value, or the mark of a missing one with its reason in its tooltip.
/// A missing value is never shown as 0.
fn value(ui: &mut Ui, value: Value<String>) -> Response {
    match value {
        Ok(text) => ui.label(RichText::new(text).strong()),
        Err(missing) => ui.label(text::MISSING).on_hover_text(reason(missing)),
    }
}

/// A name and its values among the texts below the grid, which wrap.
fn row(ui: &mut Ui, name: &str, values: Vec<Value<String>>) -> Vec<Response> {
    row_about(ui, name, None, values)
}

/// A row whose name tells, under the pointer, `about` (D50).
fn row_about(
    ui: &mut Ui,
    name: &str,
    about_name: Option<String>,
    values: Vec<Value<String>>,
) -> Vec<Response> {
    // A row that does not fit in what is left of the current line starts the
    // next, because egui's wrapping does not move a group.
    let font = egui::TextStyle::Body.resolve(ui.style());
    let words = std::iter::once(name).chain(values.iter().map(|v| match v {
        Ok(text) => text.as_str(),
        Err(_) => text::MISSING,
    }));
    let width: f32 = words
        .map(|w| {
            let galley = ui
                .painter()
                .layout_no_wrap(w.to_string(), font.clone(), Color32::WHITE);
            galley.size().x + ui.spacing().item_spacing.x
        })
        .sum();
    if width > ui.available_size_before_wrap().x {
        ui.end_row();
    }
    let responses = ui
        .horizontal(|ui| {
            about(ui.label(name), about_name);
            values.into_iter().map(|v| value(ui, v)).collect()
        })
        .inner;
    ui.add_space(ui.spacing().item_spacing.x * 2.0);
    responses
}

fn number(value: f64, decimals: usize) -> String {
    format!("{value:.decimals$}")
}

fn percent(value: f64) -> String {
    fill(text::PERCENT, &[("value", &number(value, 0))])
}

fn watts(value: f64) -> String {
    fill(text::WATTS, &[("value", &number(value, 0))])
}

/// A part of a whole in one unit, written once after both: `5.9 of 8.0 GiB`,
/// so that the small window holds more (D38, D43).
fn out_of(slot: &str, part: String, whole: String) -> String {
    fill(
        slot,
        &[(
            "value",
            &fill(text::OUT_OF, &[("part", &part), ("whole", &whole)]),
        )],
    )
}

fn gib_number(bytes: u64) -> String {
    number(bytes as f64 / (1u64 << 30) as f64, 1)
}

fn celsius(value: f64) -> String {
    fill(text::CELSIUS, &[("value", &number(value, 0))])
}

fn mhz(value: f64) -> String {
    fill(text::MHZ, &[("value", &number(value, 0))])
}

/// Bytes per second, in KiB/s below 1 MiB/s and in MiB/s above.
fn rate(bytes_per_second: f64) -> String {
    let (slot, divisor) = rate_unit(bytes_per_second);
    fill(slot, &[("value", &number(bytes_per_second / divisor, 1))])
}

/// A pair of rates, with what the pair is in its tooltip, and why a missing
/// half is missing (D18).
fn pair_value(ui: &mut Ui, name: &str, first: Value<f64>, second: Value<f64>) {
    let response = value(ui, rate_pair(first, second));
    if first.is_ok() && second.is_ok() {
        response.on_hover_text(name);
    } else if first.is_ok() || second.is_ok() {
        let missing = first.err().or(second.err()).unwrap();
        response.on_hover_text(format!("{name}\n{}", reason(missing)));
    }
}

/// Two rates in the unit of the larger, written once: `0.8 / 0.7 KiB/s`. A
/// missing one shows its mark, and the value's tooltip says why.
fn rate_pair(first: Value<f64>, second: Value<f64>) -> Value<String> {
    if first.is_err() && second.is_err() {
        return first.map(rate);
    }
    let highest = first.unwrap_or(0.0).max(second.unwrap_or(0.0));
    let (slot, divisor) = rate_unit(highest);
    let one = |v: Value<f64>| v.map_or(text::MISSING.to_string(), |v| number(v / divisor, 1));
    let pair = fill(
        text::PAIR,
        &[("first", &one(first)), ("second", &one(second))],
    );
    Ok(fill(slot, &[("value", &pair)]))
}

fn rate_unit(bytes_per_second: f64) -> (&'static str, f64) {
    if bytes_per_second < (1 << 20) as f64 {
        (text::KIB_PER_S, 1024.0)
    } else {
        (text::MIB_PER_S, (1 << 20) as f64)
    }
}

fn share(a: u64, b: u64) -> Value<f64> {
    if b == 0 {
        Err(Missing::NotReported)
    } else {
        Ok(a as f64 / b as f64 * 100.0)
    }
}

/// A time before now, in seconds, as minutes and seconds.
fn ago(seconds_before: f64) -> String {
    fill(
        text::AGO,
        &[(
            "time",
            &minutes_seconds(Duration::from_secs_f64((-seconds_before).max(0.0))),
        )],
    )
}

fn minutes_seconds(time: Duration) -> String {
    let seconds = time.as_secs_f64().round() as u64;
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

/// What one series takes from each entry: `None` where the entry has no such
/// series, such as a disk that was not mounted then.
type ValueOf<'a> = dyn Fn(&Entry) -> Option<Value<f64>> + 'a;

struct Series<'a> {
    name: &'static str,
    color: Color32,
    value: Box<ValueOf<'a>>,
}

/// A small chart of the shown history, as tall as its line.
struct Chart<'a> {
    series: Vec<Series<'a>>,
    /// The value at the chart's top. With none, the highest value shown.
    top: Option<f64>,
    /// A level drawn as a dashed line, as the threshold, with its name.
    level: Option<(&'static str, f64)>,
    format: fn(f64) -> String,
}

/// Draws `chart` across the rest of the line. A stretch with no readings is
/// a marked region, never a line (D17). Under the pointer, a tooltip gives
/// each series' value then, and how long ago.
fn draw_chart(ui: &mut Ui, view: &View, chart: Chart) {
    let width = ui.available_width().max(LINE_HEIGHT);
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, LINE_HEIGHT), Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 2.0, ui.visuals().extreme_bg_color);
    let span = view.span.as_secs_f64();
    let x = |t: f64| rect.left() + ((t + span) / span) as f32 * rect.width();
    let stretches = view.history.stretches(view.span);
    for &(start, end) in &stretches {
        let region = Rect::from_x_y_ranges(x(start)..=x(end), rect.y_range());
        painter.rect_filled(region, 0.0, GREY.gamma_multiply(0.3));
    }
    let lines: Vec<(&Series, Vec<Vec<[f64; 2]>>)> = (chart.series.iter())
        .map(|s| (s, view.history.series(view.span, width as usize, &s.value)))
        .collect();
    let highest = (lines.iter())
        .flat_map(|(_, l)| l.iter().flatten())
        .map(|p| p[1])
        .chain(chart.level.map(|(_, level)| level))
        .fold(0.0, f64::max);
    let top = chart.top.unwrap_or(highest).max(f64::MIN_POSITIVE);
    let y = |v: f64| rect.bottom() - 1.0 - (v / top).clamp(0.0, 1.0) as f32 * (rect.height() - 2.0);
    if let Some((_, level)) = chart.level {
        let line = [
            Pos2::new(rect.left(), y(level)),
            Pos2::new(rect.right(), y(level)),
        ];
        painter.extend(Shape::dashed_line(&line, Stroke::new(1.0, GREEN), 3.0, 2.0));
    }
    for (series, segments) in &lines {
        for segment in segments {
            let points: Vec<Pos2> = (segment.iter())
                .map(|p| Pos2::new(x(p[0]), y(p[1])))
                .collect();
            if let [point] = points[..] {
                painter.circle_filled(point, 1.0, series.color);
            } else {
                painter.add(Shape::line(points, Stroke::new(1.2, series.color)));
            }
        }
    }
    let Some(pointer) = response.hover_pos() else {
        return;
    };
    let t = ((pointer.x - rect.left()) / rect.width()) as f64 * span - span;
    let mut tooltip: Vec<String> = Vec::new();
    if stretches
        .iter()
        .any(|&(start, end)| (start..=end).contains(&t))
    {
        tooltip.push(text::NO_READINGS.to_string());
    } else {
        for (series, segments) in &lines {
            let nearest = (segments.iter().flatten())
                .min_by(|a, b| (a[0] - t).abs().total_cmp(&(b[0] - t).abs()))
                .filter(|p| (p[0] - t).abs() <= span / width as f64 * 2.0);
            let shown = nearest.map_or(text::MISSING.to_string(), |p| (chart.format)(p[1]));
            tooltip.push(format!("{} {shown}", series.name));
        }
        if let Some((name, level)) = chart.level {
            tooltip.push(format!("{name} {}", (chart.format)(level)));
        }
    }
    tooltip.push(ago(t));
    response.on_hover_text_at_pointer(tooltip.join("\n"));
}

/// One line of the grid: its name, its value and its chart, each of the last
/// two hidden by its box (D26). A line with both hidden is left out.
fn line(
    ui: &mut Ui,
    view: &View,
    shown: (bool, bool),
    name: impl FnOnce(&mut Ui) -> Response,
    value: impl FnOnce(&mut Ui),
    chart: Chart,
) -> Option<Response> {
    let (line_shown, chart_shown) = shown;
    if !line_shown && !chart_shown {
        return None;
    }
    let name = name(ui);
    if line_shown {
        value(ui);
    } else {
        ui.label("");
    }
    if chart_shown {
        draw_chart(ui, view, chart);
    } else {
        ui.label("");
    }
    ui.end_row();
    Some(name)
}

fn gpu_of<'a>(entry: &'a Entry, id: &str) -> Option<&'a GpuReading> {
    match &entry.reading.gpu {
        GpuState::Running(gpus) => gpus.iter().find(|g| g.id == id),
        GpuState::NotStarted { .. } => None,
    }
}

/// Each GPU's name. GPUs of the same model are numbered, so that two GPUs,
/// and their entries in the menu, are never the same.
fn gpu_titles(gpus: &[GpuReading]) -> Vec<String> {
    (gpus.iter().enumerate())
        .map(|(i, gpu)| {
            let same = |g: &&GpuReading| g.name == gpu.name;
            if gpus.iter().filter(same).count() == 1 {
                return fill(text::GPU, &[("name", &gpu.name)]);
            }
            let n = gpus[..=i].iter().filter(same).count();
            fill(
                text::GPU_NUMBERED,
                &[("name", &gpu.name), ("n", &n.to_string())],
            )
        })
        .collect()
}

fn failure_sentence(failure: GpuFailure) -> &'static str {
    match failure {
        GpuFailure::NoLibrary => text::GPU_NO_DRIVER,
        GpuFailure::DriverNotLoaded => text::GPU_DRIVER_NOT_RUNNING,
        GpuFailure::VersionMismatch => text::GPU_VERSION_MISMATCH,
        GpuFailure::NoPermission => text::GPU_NO_PERMISSION,
        GpuFailure::Lost => text::GPU_LOST,
        GpuFailure::Other => text::GPU_OTHER_FAILURE,
    }
}

/// The name of one of a GPU's lines: with several GPUs, numbered by the GPU.
fn gpu_line(name: &str, several: bool, n: usize) -> String {
    if several {
        fill(
            text::NUMBERED_LINE,
            &[("name", name), ("n", &n.to_string())],
        )
    } else {
        name.to_string()
    }
}

/// Every measurement: the sentences of what is missing, the grid of lines,
/// then the texts below it.
fn measurements(ui: &mut Ui, view: &View, settings: &Settings) {
    let Some(entry) = view.newest else {
        ui.label(text::FIRST_READING);
        return;
    };
    let gpus = running_gpus(view);
    let titles = gpu_titles(gpus);
    if let GpuState::NotStarted { failure, cards } = &entry.reading.gpu {
        // Each card is named, and the reason they share is said once.
        for address in cards {
            ui.label(RichText::new(fill(text::GPU_UNREAD, &[("address", address)])).strong());
        }
        if !cards.is_empty() {
            warning(ui, failure_sentence(*failure));
        }
    }
    for (gpu, title) in gpus.iter().zip(&titles) {
        if let Some(failure) = gpu.failure {
            warning(
                ui,
                &fill(
                    text::PART_FAILED,
                    &[("part", title), ("sentence", failure_sentence(failure))],
                ),
            );
        }
    }
    if entry.reading.cpu.is_err() {
        warning(
            ui,
            &fill(
                text::PART_FAILED,
                &[("part", text::CPU), ("sentence", text::READ_FAILED)],
            ),
        );
    }
    if entry.reading.memory.is_err() {
        warning(
            ui,
            &fill(
                text::PART_FAILED,
                &[
                    ("part", text::MEMORY_PANEL),
                    ("sentence", text::READ_FAILED),
                ],
            ),
        );
    }
    if entry.reading.disks.is_empty() {
        warning(ui, text::NO_DISKS);
    }
    if entry.reading.networks.is_empty() {
        warning(ui, text::NO_NETWORK);
    }
    egui::Grid::new("lines")
        .num_columns(3)
        .spacing([4.0, 1.0])
        .min_row_height(LINE_HEIGHT)
        .show(ui, |ui| {
            for (n, (gpu, title)) in gpus.iter().zip(&titles).enumerate() {
                gpu_lines(ui, view, gpu, title, (gpus.len() > 1, n + 1), settings);
            }
            cpu_line(ui, view, entry, settings);
            memory_line(ui, view, entry, settings);
            for (disk, rates) in entry.reading.disks.iter().zip(&entry.derived.disk_rates) {
                let device = disk.device.as_str();
                let shown = |item| settings.shown(&key(&disk_key(device), item));
                let rates_of = move |e: &Entry| {
                    let i = e.reading.disks.iter().position(|d| d.device == device)?;
                    e.derived.disk_rates.get(i).copied()
                };
                let name = line(
                    ui,
                    view,
                    (shown("rates"), shown("rates-chart")),
                    |ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 3.0;
                            let name = ui.label(disk_name(device));
                            if shown("space") {
                                space_pie(ui, disk);
                            }
                            name
                        })
                        .inner
                    },
                    |ui| {
                        pair_value(ui, text::DISK_RATES, rates.read, rates.written);
                    },
                    Chart {
                        series: vec![
                            Series {
                                name: text::READ,
                                color: BLUE,
                                value: Box::new(move |e| rates_of(e).map(|r| r.read)),
                            },
                            Series {
                                name: text::WRITTEN,
                                color: ORANGE,
                                value: Box::new(move |e| rates_of(e).map(|r| r.written)),
                            },
                        ],
                        top: None,
                        level: None,
                        format: rate,
                    },
                );
                if let Some(name) = name {
                    name.on_hover_text(disk_about(disk));
                }
            }
            for (net, rates) in entry
                .reading
                .networks
                .iter()
                .zip(&entry.derived.network_rates)
            {
                let interface = net.interface.as_str();
                let shown = |item| settings.shown(&key(&network_key(interface), item));
                let rates_of = move |e: &Entry| {
                    let i = (e.reading.networks.iter()).position(|n| n.interface == interface)?;
                    e.derived.network_rates.get(i).copied()
                };
                line(
                    ui,
                    view,
                    (shown("rates"), shown("rates-chart")),
                    |ui| {
                        let name = ui.label(fill(text::INTERFACE, &[("name", interface)]));
                        about(name, network_about(net))
                    },
                    |ui| {
                        pair_value(ui, text::NETWORK_RATES, rates.received, rates.transmitted);
                    },
                    Chart {
                        series: vec![
                            Series {
                                name: text::RECEIVED,
                                color: BLUE,
                                value: Box::new(move |e| rates_of(e).map(|r| r.received)),
                            },
                            Series {
                                name: text::TRANSMITTED,
                                color: ORANGE,
                                value: Box::new(move |e| rates_of(e).map(|r| r.transmitted)),
                            },
                        ],
                        top: None,
                        level: None,
                        format: rate,
                    },
                );
            }
        });
    ui.horizontal_wrapped(|ui| {
        for (n, (gpu, title)) in gpus.iter().zip(&titles).enumerate() {
            gpu_texts(ui, view, gpu, title, (gpus.len() > 1, n + 1), settings);
        }
        let shown = |item| settings.shown(&key("cpu", item));
        if shown("temperature") {
            let sensor = (entry.reading.cpu_sensor).map(|s| fill(text::SENSOR, &[("sensor", s)]));
            row_about(
                ui,
                text::CPU_TEMPERATURE,
                sensor,
                vec![entry.reading.cpu_temperature.map(celsius)],
            );
        }
        if shown("frequency") {
            row(
                ui,
                text::FREQUENCY,
                vec![entry.reading.cpu_frequency.map(mhz)],
            );
        }
        if settings.shown(&key("memory", "swap")) {
            let memory = entry.reading.memory;
            match memory {
                Ok(m) if m.swap_total == 0 => {
                    row(ui, text::SWAP, vec![Ok(text::NO_SWAP.to_string())]);
                }
                _ => {
                    let amounts = memory.map(|m| {
                        out_of(text::GIB, gib_number(m.swap_used), gib_number(m.swap_total))
                    });
                    row(ui, text::SWAP, vec![amounts]);
                }
            }
        }
    });
}

fn gpu_lines(
    ui: &mut Ui,
    view: &View,
    gpu: &GpuReading,
    title: &str,
    (several, n): (bool, usize),
    settings: &Settings,
) {
    let device = gpu_key(gpu);
    let shown = |item| settings.shown(&key(&device, item));
    let id = gpu.id.as_str();
    let name = line(
        ui,
        view,
        (shown("utilization"), shown("utilization-chart")),
        |ui| ui.label(gpu_line(text::UTILIZATION, several, n)),
        |ui| {
            value(ui, gpu.utilization.map(percent));
        },
        Chart {
            series: vec![Series {
                name: text::UTILIZATION,
                color: BLUE,
                value: Box::new(move |e| gpu_of(e, id).map(|g| g.utilization)),
            }],
            top: Some(100.0),
            level: None,
            format: percent,
        },
    );
    let mut names = vec![name];
    names.push(line(
        ui,
        view,
        (shown("vram"), shown("vram-chart")),
        |ui| ui.label(gpu_line(text::VRAM, several, n)),
        |ui| {
            let amounts = gpu.memory_used.and_then(|u| {
                Ok(out_of(
                    text::GIB,
                    gib_number(u),
                    gib_number(gpu.memory_total?),
                ))
            });
            value(ui, amounts);
        },
        Chart {
            series: vec![Series {
                name: text::VRAM,
                color: BLUE,
                value: Box::new(move |e| {
                    gpu_of(e, id).map(|g| g.memory_used.and_then(|u| share(u, g.memory_total?)))
                }),
            }],
            top: Some(100.0),
            level: None,
            format: percent,
        },
    ));
    let power = view
        .newest
        .and_then(|e| history::gpu_power_of(e, id))
        .unwrap_or(Err(Missing::Waiting));
    names.push(line(
        ui,
        view,
        (shown("power"), shown("power-chart")),
        |ui| ui.label(gpu_line(text::POWER, several, n)),
        |ui| power_value(ui, gpu, power),
        Chart {
            series: vec![Series {
                name: text::POWER,
                color: BLUE,
                value: Box::new(move |e| history::gpu_power_of(e, id).map(|p| p.map(|p| p.watts))),
            }],
            top: gpu.power_limit.ok(),
            level: threshold_of(view, settings, id).map(|t| (text::THRESHOLD, t)),
            format: watts,
        },
    ));
    // With several GPUs, each numbered line names its card, and tells what
    // it is (D50).
    if several {
        let title = lines([Some(title.to_string()), gpu_about(gpu)]);
        for name in names.into_iter().flatten() {
            about(name, title.clone());
        }
    }
}

/// The power and the limit it is held to, with how the power is measured in
/// its tooltip.
fn power_value(ui: &mut Ui, gpu: &GpuReading, power: Value<GpuPower>) {
    let text = power.map(|p| match gpu.power_limit {
        Ok(limit) => out_of(text::WATTS, number(p.watts, 0), number(limit, 0)),
        Err(_) => watts(p.watts),
    });
    let response = value(ui, text);
    if let Ok(power) = power {
        let tooltip = match power.averaged_over {
            Some(window) => fill(
                text::AVERAGED_OVER,
                &[("seconds", &number(window.as_secs_f64(), 1))],
            ),
            None => text::INSTANTANEOUS.to_string(),
        };
        about(response, lines([Some(tooltip), power_limits(gpu)]));
    }
}

/// The threshold the person set for the GPU, else its default (D30).
fn threshold_of(view: &View, settings: &Settings, id: &str) -> Option<f64> {
    (settings.thresholds.get(id).copied()).or(view.history.default_threshold(id))
}

/// A GPU's texts below the grid: the share of time at or above its threshold
/// (D30, D46), its temperature, fans and clock.
fn gpu_texts(
    ui: &mut Ui,
    view: &View,
    gpu: &GpuReading,
    _title: &str,
    (several, n): (bool, usize),
    settings: &Settings,
) {
    let device = gpu_key(gpu);
    let shown = |item| settings.shown(&key(&device, item));
    if shown("over-threshold") {
        let name = gpu_line(text::OVER_THRESHOLD, several, n);
        let share = match threshold_of(view, settings, &gpu.id) {
            Some(threshold) => view
                .history
                .share_over(&gpu.id, threshold, view.span)
                .map(|s| {
                    fill(
                        text::SHARE_AT,
                        &[
                            ("share", &fill(text::PERCENT, &[("value", &number(s, 1))])),
                            ("threshold", &watts(threshold)),
                        ],
                    )
                }),
            None => Ok(text::SET_THRESHOLD.to_string()),
        };
        let responses = row(ui, &name, vec![share]);
        responses[0]
            .clone()
            .on_hover_text(text::OVER_THRESHOLD_HELP);
    }
    if shown("temperature") {
        let name = gpu_line(text::GPU_TEMPERATURE, several, n);
        let limits = (gpu.info.slowdown.zip(gpu.info.shutdown)).map(|(slowdown, shutdown)| {
            fill(
                text::TEMPERATURE_LIMITS,
                &[
                    ("slowdown", &celsius(slowdown)),
                    ("shutdown", &celsius(shutdown)),
                ],
            )
        });
        row_about(ui, &name, limits, vec![gpu.temperature.map(celsius)]);
    }
    if shown("fans") {
        match &gpu.fans {
            Ok(fans) => {
                let name = gpu_line(text::FANS, several, n);
                row(ui, &name, fans.iter().map(|f| f.map(percent)).collect());
            }
            Err(missing) => {
                row(ui, &gpu_line(text::FANS, several, n), vec![Err(*missing)]);
            }
        }
    }
    if shown("clock") {
        let clock = gpu
            .clock
            .and_then(|c| Ok(out_of(text::MHZ, number(c, 0), number(gpu.max_clock?, 0))));
        row(ui, &gpu_line(text::CLOCK, several, n), vec![clock]);
    }
}

/// The field that sets a GPU's threshold (D30), in the settings' menu. The
/// threshold is set only by the person's edit: the field is bound to a copy,
/// and egui changing the copy by itself, as when it clamps it into the
/// field's range, never sets it. A drag, an edit, or a screen reader's
/// request is the person's.
fn threshold_field(
    ui: &mut Ui,
    view: &View,
    gpu: &GpuReading,
    title: &str,
    settings: &mut Settings,
) {
    let threshold = threshold_of(view, settings, &gpu.id);
    ui.horizontal(|ui| {
        let label = ui.label(fill(text::THRESHOLD_FOR, &[("name", title)]));
        let highest = gpu.power_limit_range.map_or(1000.0, |(_, highest)| highest);
        let mut copy = threshold.unwrap_or(1.0);
        // With no threshold, the field shows the mark of a missing value, not
        // its lowest value, which would read as a threshold in effect (D18).
        let unset = threshold.is_none();
        let field = ui
            .add(
                egui::DragValue::new(&mut copy)
                    .range(1.0..=highest)
                    .speed(1.0)
                    .custom_formatter(move |v, _| {
                        if unset {
                            text::MISSING.to_string()
                        } else {
                            watts(v)
                        }
                    })
                    .custom_parser(last_number),
            )
            .labelled_by(label.id)
            .on_hover_text(text::THRESHOLD_HELP);
        let by_person = field.dragged()
            || field.has_focus()
            || field.lost_focus()
            || ui.input(|i| {
                use egui::accesskit::Action;
                [Action::SetValue, Action::Increment, Action::Decrement]
                    .into_iter()
                    .any(|action| i.has_accesskit_action_request(field.id, action))
            });
        if field.changed() && by_person {
            settings.thresholds.insert(gpu.id.clone(), copy.round());
        }
    });
}

/// The last word of `text` that is a number, so that the field reads what
/// the person typed whether or not they kept the unit.
fn last_number(text: &str) -> Option<f64> {
    text.split_whitespace()
        .rev()
        .find_map(|word| word.parse().ok())
}

/// The CPU's line: its value, a bar per logical CPU after it, and its chart.
fn cpu_line(ui: &mut Ui, view: &View, entry: &Entry, settings: &Settings) {
    let shown = |item| settings.shown(&key("cpu", item));
    line(
        ui,
        view,
        (shown("usage") || shown("cores"), shown("usage-chart")),
        |ui| about(ui.label(text::USAGE), cpu_about(&entry.reading.cpu_info)),
        |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                if shown("usage") {
                    value(ui, entry.derived.cpu_usage.map(percent));
                }
                if shown("cores") && !entry.derived.core_usage.is_empty() {
                    core_bars(ui, &entry.derived.core_usage);
                }
            });
        },
        Chart {
            series: vec![Series {
                name: text::USAGE,
                color: BLUE,
                value: Box::new(|e| Some(e.derived.cpu_usage)),
            }],
            top: Some(100.0),
            level: None,
            format: percent,
        },
    );
}

/// A thin bar per logical CPU, its height the CPU's load (D13). Its tooltip
/// gives each one's value.
fn core_bars(ui: &mut Ui, cores: &[Value<f64>]) {
    let gap = 1.0;
    let bar = ((CORES_WIDTH + gap) / cores.len() as f32 - gap).clamp(1.0, 4.0);
    let width = cores.len() as f32 * (bar + gap) - gap;
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(width, LINE_HEIGHT - 2.0), Sense::hover());
    let painter = ui.painter_at(rect);
    for (i, usage) in cores.iter().enumerate() {
        let left = rect.left() + i as f32 * (bar + gap);
        let column = Rect::from_x_y_ranges(left..=left + bar, rect.y_range());
        painter.rect_filled(column, 0.0, ui.visuals().extreme_bg_color);
        if let Ok(usage) = usage {
            let top = rect.bottom() - (*usage / 100.0).clamp(0.0, 1.0) as f32 * rect.height();
            painter.rect_filled(
                Rect::from_x_y_ranges(left..=left + bar, top..=rect.bottom()),
                0.0,
                BLUE,
            );
        }
    }
    let list: Vec<String> = (cores.iter().enumerate())
        .map(|(i, usage)| {
            let name = fill(text::CORE, &[("n", &(i + 1).to_string())]);
            let shown = usage.map_or(text::MISSING.to_string(), percent);
            format!("{name} {shown}")
        })
        .collect();
    let described = format!("{}\n{}", text::CORES, list.join("\n"));
    // A screen reader reads the bars as the list of their values.
    let label = described.clone();
    response
        .on_hover_text(described)
        .widget_info(move || egui::WidgetInfo::labeled(egui::accesskit::Role::Image, true, &label));
}

/// A very small pie of how full a disk is, its color by how full (D48). Its
/// numbers are in its tooltip, and are what a screen reader reads.
fn space_pie(ui: &mut Ui, disk: &crate::reading::DiskReading) {
    let used = disk.size.saturating_sub(disk.available);
    let share = share(used, disk.size).unwrap_or(0.0);
    let diameter = LINE_HEIGHT - 2.0;
    let (rect, response) = ui.allocate_exact_size(egui::vec2(diameter, diameter), Sense::hover());
    let painter = ui.painter_at(rect.expand(1.0));
    let color = space_color(share);
    let center = rect.center();
    let radius = diameter / 2.0;
    painter.circle_filled(center, radius, ui.visuals().extreme_bg_color);
    // The used part, clockwise from the top, as a fan of thin triangles.
    let steps = ((share / 100.0 * 48.0).ceil() as usize).max(1);
    let angle = |i: usize| (i as f64 / steps as f64 * share / 100.0 * std::f64::consts::TAU) as f32;
    let at = |a: f32| center + radius * egui::vec2(a.sin(), -a.cos());
    for i in 0..steps {
        painter.add(Shape::convex_polygon(
            vec![center, at(angle(i)), at(angle(i + 1))],
            color,
            Stroke::NONE,
        ));
    }
    painter.circle_stroke(center, radius, Stroke::new(1.0, color));
    let amount = out_of(text::GIB, gib_number(used), gib_number(disk.size));
    let described = format!(
        "{}\n{}\n{}",
        disk_title(disk),
        fill(
            text::SPACE_FULL,
            &[("amount", &amount), ("share", &percent(share))]
        ),
        text::SPACE_HELP
    );
    let label = described.clone();
    response
        .on_hover_text(described)
        .widget_info(move || egui::WidgetInfo::labeled(egui::accesskit::Role::Image, true, &label));
}

/// The RAM's line, with swap as a second series of its chart.
fn memory_line(ui: &mut Ui, view: &View, entry: &Entry, settings: &Settings) {
    let shown = |item| settings.shown(&key("memory", item));
    let memory = entry.reading.memory;
    line(
        ui,
        view,
        (shown("memory"), shown("memory-chart")),
        |ui| ui.label(text::MEMORY),
        |ui| {
            value(
                ui,
                memory.map(|m| out_of(text::GIB, gib_number(m.used), gib_number(m.total))),
            );
        },
        Chart {
            series: vec![
                Series {
                    name: text::MEMORY,
                    color: BLUE,
                    value: Box::new(|e| {
                        Some(e.reading.memory.and_then(|m| share(m.used, m.total)))
                    }),
                },
                Series {
                    name: text::SWAP,
                    color: ORANGE,
                    value: Box::new(|e| {
                        Some(
                            e.reading
                                .memory
                                .and_then(|m| share(m.swap_used, m.swap_total)),
                        )
                    }),
                },
            ],
            top: Some(100.0),
            level: None,
            format: percent,
        },
    );
}

#[cfg(test)]
mod tests;
