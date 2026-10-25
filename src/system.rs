//! Reads the CPU, memory, disks, network and the CPU temperature.
//!
//! CPU times come from `/proc/stat` rather than `sysinfo`, because `sysinfo`
//! gives only a percentage of its own, with no cumulative times, and skips a
//! refresh that comes less than 200 ms after its last one, which would
//! repeat an old value as new at slm's fastest refresh.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use sysinfo::{Components, CpuRefreshKind, Disks, MemoryRefreshKind, Networks, System};

use crate::reading::{
    CpuInfo, CpuStat, CpuTimes, DiskInfo, DiskReading, Memory, Missing, NetworkInfo,
    NetworkReading, Value,
};

/// What the system part of one reading holds.
pub struct SystemReading {
    pub cpu: Value<CpuStat>,
    pub cpu_frequency: Value<f64>,
    pub cpu_temperature: Value<f64>,
    pub cpu_sensor: Option<&'static str>,
    pub cpu_info: Arc<CpuInfo>,
    pub memory: Value<Memory>,
    pub disks: Vec<DiskReading>,
    pub networks: Vec<NetworkReading>,
}

pub struct SystemSource {
    system: System,
    disks: Disks,
    networks: Networks,
    components: Components,
    // What each device is, read once, by its name (D50).
    cpu_info: Option<Arc<CpuInfo>>,
    disk_info: HashMap<String, Arc<DiskInfo>>,
    network_info: HashMap<String, Arc<NetworkInfo>>,
}

impl SystemSource {
    pub fn new() -> Self {
        Self {
            system: System::new(),
            disks: Disks::new(),
            networks: Networks::new(),
            components: Components::new(),
            cpu_info: None,
            disk_info: HashMap::new(),
            network_info: HashMap::new(),
        }
    }

    pub fn read(&mut self) -> SystemReading {
        self.system
            .refresh_cpu_specifics(CpuRefreshKind::nothing().with_frequency());
        self.system
            .refresh_memory_specifics(MemoryRefreshKind::everything());
        self.disks.refresh(true);
        self.networks.refresh(true);
        self.components.refresh(true);

        let cpu = std::fs::read_to_string("/proc/stat")
            .ok()
            .and_then(|text| parse_proc_stat(&text))
            .ok_or(Missing::Failed);

        let cpus = self.system.cpus();
        let cpu_info = (self.cpu_info.get_or_insert_with(|| {
            Arc::new(CpuInfo {
                model: (cpus.first().map(|c| c.brand().trim().to_string()))
                    .filter(|b| !b.is_empty()),
                cores: System::physical_core_count(),
                logical: (!cpus.is_empty()).then_some(cpus.len()),
            })
        }))
        .clone();
        let cpu_frequency = if cpus.is_empty() || cpus.iter().all(|c| c.frequency() == 0) {
            Err(Missing::NotReported)
        } else {
            Ok(cpus.iter().map(|c| c.frequency() as f64).sum::<f64>() / cpus.len() as f64)
        };

        let sensors: Vec<(&str, Option<f32>)> = (self.components.list().iter())
            .map(|c| (c.label(), c.temperature()))
            .collect();

        let memory = match self.system.total_memory() {
            0 => Err(Missing::Failed),
            total => Ok(Memory {
                used: self.system.used_memory(),
                total,
                swap_used: self.system.used_swap(),
                swap_total: self.system.total_swap(),
            }),
        };

        let diskstats = std::fs::read_to_string("/proc/diskstats").unwrap_or_default();
        let counted = counted_devices(&diskstats);
        let disk_info = &mut self.disk_info;
        let disks = group_disks(self.disks.list().iter().map(|d| {
            let device = d.name().to_string_lossy().into_owned();
            let info = (disk_info.entry(device.clone()).or_insert_with(|| {
                Arc::new(DiskInfo {
                    model: disk_model(&kernel_name(&device)),
                    solid_state: match d.kind() {
                        sysinfo::DiskKind::SSD => Some(true),
                        sysinfo::DiskKind::HDD => Some(false),
                        sysinfo::DiskKind::Unknown(_) => None,
                    },
                    file_system: Some(d.file_system().to_string_lossy().into_owned())
                        .filter(|f| !f.is_empty()),
                })
            }))
            .clone();
            MountedDisk {
                info,
                counted: counted.contains(kernel_name(&device).as_str()),
                device,
                mount_point: d.mount_point().to_path_buf(),
                size: d.total_space(),
                available: d.available_space(),
                read: d.usage().total_read_bytes,
                written: d.usage().total_written_bytes,
            }
        }));

        let sys_class_net = Path::new("/sys/class/net");
        let network_info = &mut self.network_info;
        let mut networks: Vec<NetworkReading> = (self.networks.list().iter())
            .filter(|(name, _)| is_physical(sys_class_net, name))
            .map(|(name, data)| {
                let adapter = sys_class_net.join(name);
                let read = |file: &str| std::fs::read_to_string(adapter.join(file)).ok();
                NetworkReading {
                    interface: name.clone(),
                    received: data.total_received(),
                    transmitted: data.total_transmitted(),
                    link: link(read("speed").as_deref(), read("duplex").as_deref()),
                    info: (network_info.entry(name.clone()).or_insert_with(|| {
                        let udev = (read("ifindex"))
                            .and_then(|i| {
                                std::fs::read_to_string(format!("/run/udev/data/n{}", i.trim()))
                                    .ok()
                            })
                            .unwrap_or_default();
                        Arc::new(NetworkInfo {
                            model: udev_model(&udev),
                            driver: std::fs::read_link(adapter.join("device/driver"))
                                .ok()
                                .and_then(|p| Some(p.file_name()?.to_string_lossy().into_owned())),
                        })
                    }))
                    .clone(),
                }
            })
            .collect();
        networks.sort_by(|a, b| a.interface.cmp(&b.interface));

        SystemReading {
            cpu,
            cpu_frequency,
            cpu_temperature: cpu_temperature(&sensors),
            cpu_sensor: cpu_sensor(&sensors),
            cpu_info,
            memory,
            disks,
            networks,
        }
    }
}

/// Parses the `cpu` lines of `/proc/stat`. Time spent idle or waiting for
/// input or output is not busy. Guest time is already counted in user time,
/// so it is left out of the total.
pub fn parse_proc_stat(text: &str) -> Option<CpuStat> {
    let mut total = None;
    let mut cores = Vec::new();
    for line in text.lines() {
        let mut fields = line.split_ascii_whitespace();
        let Some(name) = fields.next().filter(|n| n.starts_with("cpu")) else {
            continue;
        };
        let ticks: Vec<u64> = fields
            .take(8)
            .map(|f| f.parse().ok())
            .collect::<Option<_>>()?;
        if ticks.len() < 5 {
            return None;
        }
        let all: u64 = ticks.iter().sum();
        let times = CpuTimes {
            busy: all - ticks[3] - ticks[4],
            all,
        };
        if name == "cpu" {
            total = Some(times);
        } else {
            cores.push(times);
        }
    }
    Some(CpuStat {
        total: total?,
        cores,
    })
}

/// The labels `sysinfo` gives the sensors that measure the CPU package, in
/// order of preference. On CPUs whose "Tctl" carries an offset, "Tdie" is the
/// die's temperature.
const CPU_SENSORS: [&str; 3] = ["k10temp Tdie", "k10temp Tctl", "coretemp Package id 0"];

/// The label of the sensor the CPU's temperature comes from (D50).
pub fn cpu_sensor(sensors: &[(&str, Option<f32>)]) -> Option<&'static str> {
    (CPU_SENSORS.iter().copied()).find(|label| sensors.iter().any(|(l, _)| l == label))
}

pub fn cpu_temperature(sensors: &[(&str, Option<f32>)]) -> Value<f64> {
    let (_, temperature) = CPU_SENSORS
        .iter()
        .find_map(|label| sensors.iter().find(|(l, _)| l == label))
        .ok_or(Missing::NotReported)?;
    temperature.map(f64::from).ok_or(Missing::Failed)
}

pub struct MountedDisk {
    pub device: String,
    pub mount_point: PathBuf,
    pub size: u64,
    pub available: u64,
    pub read: u64,
    pub written: u64,
    /// Whether the kernel keeps read and write counters for the device. For
    /// one it does not, such as an `overlay` or a network filesystem,
    /// `sysinfo` leaves its counts at 0, which would show as a rate of 0.
    pub counted: bool,
    pub info: Arc<DiskInfo>,
}

/// One entry per device, with its mount points together, leaving out loop
/// devices (D6) and snap's mount points (D42). `sysinfo` already leaves out
/// `squashfs` and `tmpfs`.
pub fn group_disks(mounted: impl Iterator<Item = MountedDisk>) -> Vec<DiskReading> {
    let mut disks: Vec<DiskReading> = Vec::new();
    let snaps = |d: &MountedDisk| {
        ["/snap", "/var/snap"]
            .iter()
            .any(|s| d.mount_point.starts_with(s))
    };
    for disk in mounted.filter(|d| !d.device.starts_with("/dev/loop") && !snaps(d)) {
        match disks.iter_mut().find(|d| d.device == disk.device) {
            Some(entry) => entry.mount_points.push(disk.mount_point),
            None => disks.push(DiskReading {
                device: disk.device,
                mount_points: vec![disk.mount_point],
                size: disk.size,
                available: disk.available,
                read: disk
                    .counted
                    .then_some(disk.read)
                    .ok_or(Missing::NotReported),
                written: (disk.counted.then_some(disk.written)).ok_or(Missing::NotReported),
                info: disk.info,
            }),
        }
    }
    for disk in &mut disks {
        disk.mount_points.sort();
    }
    disks.sort_by(|a, b| a.device.cmp(&b.device));
    disks
}

/// The names of the devices `/proc/diskstats` counts, its third column.
pub fn counted_devices(diskstats: &str) -> HashSet<&str> {
    (diskstats.lines())
        .filter_map(|line| line.split_whitespace().nth(2))
        .collect()
}

/// The kernel's name for a device, such as `dm-0` for `/dev/mapper/root`,
/// the name `/proc/diskstats` uses, found as `sysinfo` finds it.
fn kernel_name(device: &str) -> String {
    let path = std::fs::canonicalize(device).unwrap_or_else(|_| PathBuf::from(device));
    (path.file_name()).map_or(device.to_string(), |n| n.to_string_lossy().into_owned())
}

/// The model of the drive a block device is on, such as "WDC
/// WDS500G1B0C-00S6U0", from `/sys/class/block/`. A partition's model is its
/// drive's. A device with no drive, such as `dm-0`, has none.
fn disk_model(kernel_name: &str) -> Option<String> {
    let mut path = std::fs::canonicalize(Path::new("/sys/class/block").join(kernel_name)).ok()?;
    if path.join("partition").exists() {
        path.pop();
    }
    let model = std::fs::read_to_string(path.join("device/model")).ok()?;
    Some(model.trim().to_string()).filter(|m| !m.is_empty())
}

/// A link's speed and duplex from `/sys/class/net/<name>/speed` and
/// `duplex`. A link that is down gives an error or -1, and is `None`.
pub fn link(speed: Option<&str>, duplex: Option<&str>) -> Option<(u32, bool)> {
    let speed: u32 = speed?.trim().parse().ok().filter(|s| *s > 0)?;
    Some((speed, duplex.is_some_and(|d| d.trim() == "full")))
}

/// The vendor and model udev's hardware database gives an adapter, from its
/// `/run/udev/data/n<ifindex>` file.
pub fn udev_model(udev: &str) -> Option<String> {
    let field = |name: &str| {
        (udev.lines()).find_map(|l| l.strip_prefix(&format!("E:{name}=")).map(str::trim))
    };
    let model = field("ID_MODEL_FROM_DATABASE");
    let vendor = field("ID_VENDOR_FROM_DATABASE");
    match (vendor, model) {
        (Some(vendor), Some(model)) => Some(format!("{vendor} {model}")),
        (vendor, model) => model.or(vendor).map(str::to_string),
    }
}

/// A physical interface has a `device` link under `/sys/class/net/<name>/`,
/// and loopback and virtual interfaces, such as `virbr0`, do not (D13).
pub fn is_physical(sys_class_net: &Path, interface: &str) -> bool {
    sys_class_net.join(interface).join("device").exists()
}

#[cfg(test)]
mod tests {
    use super::*;

    const PROC_STAT: &str = "\
cpu  100 10 50 800 40 5 5 0 7 0
cpu0 60 5 25 400 20 3 2 0 7 0
cpu1 40 5 25 400 20 2 3 0 0 0
intr 12345 0 0
ctxt 999
";

    #[test]
    fn d1_cpu_usage_is_the_share_of_ticks_neither_idle_nor_waiting() {
        let before = parse_proc_stat(PROC_STAT).unwrap();
        assert_eq!(
            before.total,
            CpuTimes {
                busy: 170,
                all: 1010
            }
        );
        assert_eq!(before.cores.len(), 2);
        let after = parse_proc_stat(
            "cpu  150 10 100 850 40 5 5 0 0 0\ncpu0 85 5 50 425 20 3 2 0 0 0\ncpu1 65 5 50 425 20 2 3 0 0 0\n",
        )
        .unwrap();

        let mut history = crate::history::History::default();
        for (i, cpu) in [before, after].into_iter().enumerate() {
            history.push(crate::reading::Reading {
                cpu: Ok(cpu),
                ..crate::history::tests::bare(
                    crate::history::tests::ms(500 * i as u64),
                    crate::history::tests::ms(500),
                )
            });
        }
        let derived = &history.newest().unwrap().derived;
        // 100 busy ticks of 150 for the CPU, 50 of 75 for each core.
        assert_eq!(derived.cpu_usage, Ok(100.0 / 150.0 * 100.0));
        assert_eq!(derived.core_usage, [Ok(50.0 / 75.0 * 100.0); 2]);
    }

    #[test]
    fn a_proc_stat_without_a_cpu_line_is_a_failed_read() {
        assert_eq!(parse_proc_stat("intr 1 2\n"), None);
        assert_eq!(parse_proc_stat("cpu  1 x 3 4 5\n"), None);
    }

    #[test]
    fn the_cpu_temperature_sensor_is_chosen_by_label() {
        let amd = [("acpitz temp1", Some(70.0)), ("k10temp Tctl", Some(55.5))];
        assert_eq!(cpu_temperature(&amd), Ok(55.5));
        let offset = [("k10temp Tctl", Some(75.0)), ("k10temp Tdie", Some(55.0))];
        assert_eq!(cpu_temperature(&offset), Ok(55.0));
        let intel = [
            ("coretemp Core 0", Some(40.0)),
            ("coretemp Package id 0", Some(45.0)),
        ];
        assert_eq!(cpu_temperature(&intel), Ok(45.0));
        let none = [("acpitz temp1", Some(70.0)), ("nvme Composite", Some(40.0))];
        assert_eq!(cpu_temperature(&none), Err(Missing::NotReported));
    }

    #[test]
    fn d50_a_link_that_is_down_has_no_speed() {
        assert_eq!(link(Some("1000\n"), Some("full\n")), Some((1000, true)));
        assert_eq!(link(Some("100\n"), Some("half\n")), Some((100, false)));
        assert_eq!(link(Some("-1\n"), Some("unknown\n")), None);
        assert_eq!(link(None, None), None);
    }

    #[test]
    fn d50_an_adapters_model_comes_from_udevs_database() {
        let udev = "I:123\nE:ID_VENDOR_FROM_DATABASE=Realtek Semiconductor Co., Ltd.\n\
            E:ID_MODEL_FROM_DATABASE=RTL8111/8168/8411 PCI Express Gigabit Ethernet Controller\n";
        assert_eq!(
            udev_model(udev).as_deref(),
            Some(
                "Realtek Semiconductor Co., Ltd. RTL8111/8168/8411 PCI Express Gigabit Ethernet Controller"
            )
        );
        assert_eq!(
            udev_model("E:ID_MODEL_FROM_DATABASE=X\n").as_deref(),
            Some("X")
        );
        assert_eq!(udev_model(""), None);
    }

    #[test]
    fn d50_the_sensor_named_is_the_one_the_temperature_comes_from() {
        let both = [("k10temp Tctl", Some(60.0)), ("k10temp Tdie", Some(50.0))];
        assert_eq!(cpu_sensor(&both), Some("k10temp Tdie"));
        assert_eq!(cpu_sensor(&[("acpitz temp1", Some(70.0))]), None);
    }

    fn mounted(device: &str, mount_point: &str) -> MountedDisk {
        MountedDisk {
            device: device.into(),
            mount_point: mount_point.into(),
            size: 500,
            available: 30,
            read: 7,
            written: 9,
            counted: true,
            info: Arc::default(),
        }
    }

    #[test]
    fn d18_a_disk_the_kernel_keeps_no_counters_for_has_no_rates_not_zero() {
        let overlay = MountedDisk {
            counted: false,
            ..mounted("overlay", "/var/lib/docker/overlay2/merged")
        };
        let disks = group_disks([mounted("/dev/sda1", "/data"), overlay].into_iter());
        assert_eq!(disks[0].device, "/dev/sda1");
        assert_eq!((disks[0].read, disks[0].written), (Ok(7), Ok(9)));
        assert_eq!(disks[1].device, "overlay");
        let none = Err(Missing::NotReported);
        assert_eq!((disks[1].read, disks[1].written), (none, none));
    }

    #[test]
    fn the_kernel_counts_the_devices_proc_diskstats_lists() {
        let stats = "\
 259       0 nvme0n1 100 0 2000 0 0 0 0 0 0 0 0 0 0 0 0 0 0
 259       1 nvme0n1p1 90 0 1800 0 0 0 0 0 0 0 0 0 0 0 0 0 0
 253       0 dm-0 5 0 40 0 0 0 0 0 0 0 0 0 0 0 0 0 0
";
        let names = counted_devices(stats);
        assert!(names.contains("nvme0n1p1"));
        assert!(names.contains("dm-0"));
        assert!(!names.contains("overlay"));
        assert!(!names.contains("259"));
    }

    #[test]
    fn d42_snaps_mount_points_are_left_out() {
        let disks = group_disks(
            [
                mounted("/dev/nvme0n1p1", "/"),
                mounted("/dev/nvme0n1p1", "/var/snap/firefox/common/host-hunspell"),
                mounted("/dev/sdb1", "/snap/hello/data"),
                mounted("/dev/sda1", "/snapshots"),
            ]
            .into_iter(),
        );
        let entries: Vec<_> = (disks.iter())
            .map(|d| (d.device.as_str(), d.mount_points.clone()))
            .collect();
        assert_eq!(
            entries,
            [
                ("/dev/nvme0n1p1", vec![PathBuf::from("/")]),
                ("/dev/sda1", vec![PathBuf::from("/snapshots")]),
            ]
        );
    }

    #[test]
    fn d6_disks_group_mount_points_of_one_device_and_leave_out_loop_devices() {
        let disks = group_disks(
            [
                mounted("/dev/nvme0n1p1", "/"),
                mounted("/dev/loop3", "/snap/core/1"),
                mounted("/dev/sda1", "/data"),
                mounted("/dev/nvme0n1p1", "/home"),
            ]
            .into_iter(),
        );
        let entries: Vec<_> = (disks.iter())
            .map(|d| (d.device.as_str(), d.mount_points.clone()))
            .collect();
        assert_eq!(
            entries,
            [
                (
                    "/dev/nvme0n1p1",
                    vec![PathBuf::from("/"), PathBuf::from("/home")]
                ),
                ("/dev/sda1", vec![PathBuf::from("/data")]),
            ]
        );
    }

    #[test]
    fn d13_network_interfaces_without_a_device_link_are_left_out() {
        let root = std::env::temp_dir().join(format!("slm-net-{}", std::process::id()));
        let physical = root.join("enp3s0");
        std::fs::create_dir_all(&physical).unwrap();
        std::fs::create_dir_all(root.join("pci")).unwrap();
        std::os::unix::fs::symlink(root.join("pci"), physical.join("device")).unwrap();
        std::fs::create_dir_all(root.join("virbr0")).unwrap();
        std::fs::create_dir_all(root.join("lo")).unwrap();
        let kept: Vec<_> = ["enp3s0", "virbr0", "lo"]
            .into_iter()
            .filter(|n| is_physical(&root, n))
            .collect();
        std::fs::remove_dir_all(&root).unwrap();
        assert_eq!(kept, ["enp3s0"]);
    }
}
