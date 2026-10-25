//! Every text the window shows or gives a screen reader, each in one slot,
//! so that the words can be written and changed in one place (D19, D27).
//!
//! A slot that holds values names each as `{name}`, which `fill` replaces.
//! They are written from what each place does (D19).

// The window and its settings bar.
pub const WINDOW_TITLE: &str = "slm";
pub const REFRESH: &str = "Measure every";
pub const HISTORY: &str = "Charts show last";
/// A refresh interval in milliseconds, `{n}`.
pub const MILLISECONDS: &str = "{n} ms";
/// A refresh interval in seconds, `{n}`.
pub const SECONDS: &str = "{n} s";
/// A history length in minutes, `{n}`.
pub const MINUTES: &str = "{n} min";
/// The heading of the boxes of what is shown, in the settings menu (D26, D44).
pub const SHOWN: &str = "Choose lines and charts";

// The panels' names.
pub const CPU: &str = "CPU";
pub const MEMORY_PANEL: &str = "Memory";
/// A GPU's panel, with the name the driver gives the GPU, `{name}`.
pub const GPU: &str = "{name}";
/// The panel of an NVIDIA GPU slm cannot read (D20), with its PCI address, `{address}`.
pub const GPU_UNREAD: &str = "NVIDIA GPU at {address} cannot be measured";

// The panels' states.
/// A panel before its first reading.
pub const FIRST_READING: &str = "Measuring…";
/// NVIDIA's library could not be loaded: the driver is not installed (D20).
pub const GPU_NO_DRIVER: &str = "NVIDIA's own driver is not installed (nouveau gives slm nothing to read). Install it and reboot; the GPU then appears within 10 s.";
/// The driver is installed but not running.
pub const GPU_DRIVER_NOT_RUNNING: &str = "NVIDIA's driver is installed but not loaded in the running kernel, often after a kernel update or because Secure Boot blocks it. slm retries every 10 s.";
/// The library and the kernel module come from different driver versions.
pub const GPU_VERSION_MISMATCH: &str = "NVIDIA's driver and its kernel module are different versions, usually after an update without a reboot. Reboot.";
pub const GPU_NO_PERMISSION: &str = "NVIDIA's driver denied this user access to the card. Check that this user may open NVIDIA's device files. slm retries every 10 s.";
pub const GPU_OTHER_FAILURE: &str =
    "NVIDIA's library failed to start, for an unknown reason. slm tries again every 10 s.";
/// The GPU stopped responding while slm ran.
pub const GPU_LOST: &str = "stopped answering. slm retries every 10 s and resumes by itself; if it stays silent, reset the GPU or reboot.";
/// Reading the CPU or memory failed.
pub const READ_FAILED: &str = "could not be measured; retrying at every measurement.";
pub const NO_DISKS: &str =
    "No mounted disk to show (loop, in-memory and snap mounts are left out).";
pub const NO_NETWORK: &str =
    "No network adapter to show: loopback and virtual adapters are left out.";

// What a missing value shows, and why it is missing.
pub const MISSING: &str = "—";
pub const NOT_REPORTED: &str = "This hardware or its driver does not provide this value.";
pub const WAITING: &str = "Not available yet; it appears by itself shortly.";
pub const NO_RATE: &str = "Cannot be computed across a gap in measurements or a counter reset; back at the next measurement.";
pub const FAILED: &str = "This measurement failed; slm tries again at the next one.";

// The lines and graphs of a GPU panel.
pub const UTILIZATION: &str = "GPU";
pub const VRAM: &str = "VRAM";
pub const POWER: &str = "Power";
pub const OVER_THRESHOLD: &str = "At or above threshold";
/// The line over the threshold, for a GPU that has no threshold yet (D30).
pub const SET_THRESHOLD: &str = "not set, see Settings";
pub const TEMPERATURE: &str = "Temperature";
/// The fans, in the menu of what is shown.
pub const FANS: &str = "Fans";
pub const CLOCK: &str = "GPU clock";
pub const GRAPH_GPU: &str = "GPU chart";
pub const GRAPH_VRAM: &str = "VRAM chart";
pub const GRAPH_POWER: &str = "Power chart";
/// GPU power's tooltip: averaged over `{seconds}`.
pub const AVERAGED_OVER: &str = "Average over the last {seconds} s, from the GPU's energy counter: short peaks are smoothed and changes show a little late.";
/// GPU power's tooltip, on a card without the energy counter.
pub const INSTANTANEOUS: &str =
    "Instant figure from the card, not averaged: it can jump between measurements.";

// The CPU panel.
pub const USAGE: &str = "CPU";
pub const CORES: &str = "Logical CPUs";
/// A core, by its number, `{n}`.
pub const CORE: &str = "CPU {n}";
pub const FREQUENCY: &str = "CPU clock";
pub const GRAPH_CPU: &str = "CPU chart";

// The memory panel.
pub const MEMORY: &str = "RAM";
pub const SWAP: &str = "Swap";
pub const GRAPH_MEMORY: &str = "RAM and swap chart";

// The disks and network panels.
/// A disk, by its device, `{device}`, and its mount points, `{mounts}`.
pub const DISK: &str = "{device} on {mounts}";
pub const SPACE: &str = "Space";
pub const READ: &str = "Reading";
pub const WRITTEN: &str = "Writing";
pub const GRAPH_DISK: &str = "Reading and writing chart";
/// A network interface, by its name, `{name}`.
pub const INTERFACE: &str = "{name}";
pub const RECEIVED: &str = "Receiving";
pub const TRANSMITTED: &str = "Sending";
pub const GRAPH_NETWORK: &str = "Receiving and sending chart";

// The graphs.
/// The region of a graph that has no readings, such as a suspend (D17).
pub const NO_READINGS: &str = "Not measured";
/// A time on a graph's axis or under the pointer, `{time}`, as minutes and seconds before now.
pub const AGO: &str = "{time} ago";

// Values and their units.
/// A part out of a whole, `{part}` and `{whole}`, each already with its unit.
pub const OUT_OF: &str = "{part} of {whole}";
pub const PERCENT: &str = "{value}%";
pub const WATTS: &str = "{value} W";
pub const GIB: &str = "{value} GiB";
pub const CELSIUS: &str = "{value} °C";
pub const MHZ: &str = "{value} MHz";
pub const KIB_PER_S: &str = "{value} KiB/s";
pub const MIB_PER_S: &str = "{value} MiB/s";

/// The swap line, on a machine with no swap.
pub const NO_SWAP: &str = "none";
/// A GPU's panel when another GPU has the same name, with the name, `{name}`, and its number, `{n}`.
pub const GPU_NUMBERED: &str = "{name} #{n}";
/// The tooltip of the line of the share over the threshold: how it is counted.
pub const OVER_THRESHOLD_HELP: &str = "Share of the time on the charts during which GPU power was at or above the threshold. Each interval counts by its length, judged by the power at its end; intervals without power count on neither side. Changing the threshold or the chart length recounts all of it.";
/// The tooltip of the threshold's field: its default, and that a set one stays.
pub const THRESHOLD_HELP: &str = "Threshold in watts for the share below the charts. Until you set it, it is 95% of the power limit, rounded, and follows the limit. Drag (1 W per pixel) or click and type, from 1 W up to the GPU's highest limit (1000 W if unknown). Once set, it is kept for this GPU and the 95% default cannot come back.";
/// Measuring has stopped while the window runs, for `{time}`, as minutes and
/// seconds.
pub const STOPPED: &str =
    "No measurement for {time}: values and charts are frozen. If it lasts, restart slm.";
/// The tooltip of a disk's space: that used space counts all the space users
/// cannot write to. It says no more, because btrfs has no root reserve, yet
/// its used space is above df's.
pub const SPACE_HELP: &str = "Counts all space users cannot write to, so it can be above what df shows; the size matches df.";

/// The button that opens the settings and the boxes of what is shown.
pub const SETTINGS: &str = "Settings";
/// The interval, `{refresh}`, and the span, `{span}`, in the top line.
pub const SUMMARY: &str = "{refresh} · {span}";
/// The threshold's field in the settings, for the GPU `{name}`.
pub const THRESHOLD_FOR: &str = "Threshold for {name}";
/// A GPU's line, `{name}`, when there are several GPUs, with the GPU's number, `{n}`.
pub const NUMBERED_LINE: &str = "{name} {n}";
/// Two rates, `{first}` and `{second}`, sharing one unit.
pub const PAIR: &str = "{first} · {second}";
/// A disk's line in the menu, and the tooltip of its value: reading, then writing.
pub const DISK_RATES: &str = "Reading, then writing";
/// A network adapter's line in the menu, and the tooltip of its value.
pub const NETWORK_RATES: &str = "Receiving, then sending";
/// A part, `{part}`, whose reading failed, and why, `{sentence}`.
pub const PART_FAILED: &str = "{part}: {sentence}";
pub const GPU_TEMPERATURE: &str = "GPU temp";
pub const CPU_TEMPERATURE: &str = "CPU temp";
/// The share at or above the threshold, `{share}`, and the threshold, `{threshold}`.
pub const SHARE_AT: &str = "{share} ({threshold})";

/// A disk's space used, `{amount}`, and its share, `{share}`, in its pie's
/// tooltip (D48).
pub const SPACE_FULL: &str = "{amount} ({share})";
/// The threshold, named in the power chart's tooltip.
pub const THRESHOLD: &str = "Threshold";

// The tooltips of the names (D50). Each is one line, left out when its values
// are unknown.
/// A GPU's architecture and its VRAM, such as "Turing · 8.0 GiB VRAM".
pub const GPU_KIND: &str = "{architecture} · {vram} VRAM";
/// A PCIe link, `{generation}` and `{width}` as the card reports them.
pub const PCIE: &str = "PCIe {generation}.0 ×{width}";
/// The GPU's PCIe link at most, and now, which drops while the GPU is idle.
pub const GPU_LINK: &str = "{max} at most, {now} now (lower while idle)";
/// The driver's version, and the highest CUDA version it supports.
pub const GPU_DRIVER: &str = "Driver {driver} · CUDA {cuda}";
pub const DRIVER: &str = "Driver {driver}";
/// The GPU's temperature limits.
pub const TEMPERATURE_LIMITS: &str = "Slows down at {slowdown}, shuts down at {shutdown}";
/// The power limit, its default, and the range it can be set in.
pub const POWER_LIMITS: &str = "Limit {limit}, default {default}, can be set from {min} to {max}";
/// A network link's speed, in megabits as Linux gives it and as a rate, to
/// compare with the adapter's rates.
pub const LINK: &str = "{speed} Mb/s link ({rate}), {duplex}";
pub const FULL_DUPLEX: &str = "full duplex";
pub const HALF_DUPLEX: &str = "half duplex";
/// What a disk is: SSD or HDD, and its filesystem.
pub const SSD: &str = "SSD";
pub const HDD: &str = "HDD";
/// The CPU's cores and logical CPUs.
pub const CPU_CORES: &str = "{cores} cores, {logical} logical CPUs";
/// The sensor the CPU's temperature comes from.
pub const SENSOR: &str = "Sensor: {sensor}";

/// Replaces each `{name}` in `slot` by its value.
pub fn fill(slot: &str, values: &[(&str, &str)]) -> String {
    let mut text = slot.to_string();
    for (name, value) in values {
        text = text.replace(&format!("{{{name}}}"), value);
    }
    text
}

#[cfg(test)]
mod tests {
    #[test]
    fn d19_no_placeholder_is_left() {
        let source = include_str!("text.rs");
        let code = &source[..source.find("#[cfg(test)]").unwrap()];
        assert!(!code.contains("\"[T"), "a slot still holds its placeholder");
    }
}
