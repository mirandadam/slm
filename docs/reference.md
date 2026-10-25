# slm reference

How slm is built, for people who run it and people who change it. `README.md` is the short introduction.

slm ("system load monitor") is a small Linux desktop window that shows the machine's load as live numbers and charts. It shows the CPU, memory, mounted disks, network interfaces, and NVIDIA GPUs with how busy they are, their memory, power, temperature, fans and clock. It is made for X11 and Wayland, and has so far run only on X11.

## Building and running

slm needs Rust 1.95 or newer, because eframe 0.37 and sysinfo 0.39 do.

```sh
cargo build --release
cargo run --release
```

slm reads NVIDIA GPUs through NVML, the library NVIDIA's own driver installs as `libnvidia-ml.so.1`. slm loads it when it starts, so slm also starts without the NVIDIA driver, and then says why it cannot read the GPU. The open `nouveau` driver gives slm nothing to read.

slm saves its settings ("Measure every", "Charts show last", each GPU's threshold and the boxes of what is shown, all in the menu of its "Settings" button) and the window's position in `$XDG_DATA_HOME/slm/app.ron`, or `~/.local/share/slm/app.ron` when `XDG_DATA_HOME` is not set. The window is a small one beside the work it watches: 350 px wide, as tall as what it shows, and not resizable (D49). With one GPU, one disk and one network adapter it is about 190 px tall; each further disk or adapter adds a line. eframe still saves a size, which slm overrides at its first frame.

## Where each number comes from

slm reads every source on a thread of its own, once per interval chosen in "Measure every", whether or not the window is shown. It keeps the readings of the last hour, and "Charts show last" chooses how much of that hour the charts show. Each reading is timed on `CLOCK_BOOTTIME`, which keeps counting while the machine is suspended, so a suspend shows on the charts as a region marked "Not measured", at its real length.

- **The CPU's load**, overall and for each logical CPU, is the share of the ticks counted in `/proc/stat` between two readings that were neither idle nor waiting for input or output. slm reads `/proc/stat` itself, rather than through sysinfo, because sysinfo keeps no cumulative times and skips a refresh that comes less than 200 ms after its last one.
- **CPU frequency** is the average of the cores' current frequencies, through sysinfo.
- **CPU temperature** is the `k10temp` sensor "Tdie" where it exists, else its "Tctl", else the `coretemp` sensor "Package id 0". On CPUs whose Tctl carries an offset, such as the Ryzen 7 1700X, Tdie is the die's temperature. On other CPUs, such as the Ryzen 5 2600X, Tctl is.
- **Memory and swap used** come from sysinfo, whose "used" memory is the "used" column of `free -b`.
- **Disks** are the mounted filesystems sysinfo lists, which leave out `squashfs`, the filesystem of snap packages, and `tmpfs`. slm also leaves out loop devices and the mount points snap makes under `/snap/` and `/var/snap/`, and shows one entry per device, with its mount points together. A small pie beside each disk's name shows its space used: green below 70%, yellow from 70%, red from 90% (D48); its tooltip, which a screen reader also reads, gives the amount and the share. The space used is the size less the space available to users, so it counts all the space users cannot write to, such as ext4's blocks reserved for root, and can be larger than the "Used" column of `df`. The read and write rates come from the kernel's counters of sectors read and written.
- **Network** shows the interfaces that have a `device` link in `/sys/class/net/<name>/`, which leaves out loopback and virtual interfaces, such as `virbr0`. The rates come from the cumulative bytes received and transmitted.
- **GPU** values come from NVML: the line "GPU" is NVML's utilization, then the memory used, the enforced power limit (the limit the card holds to, not the limit requested by software, which `nvidia-smi` calls `power.limit`), temperature, each fan's speed, and the graphics clock against its maximum.

**What a pointer on a name tells** (D50) is read once per device, when slm first sees it, except the two links, which are read at each measurement because they change:

- the GPU's architecture, its default power limit, its slowdown and shutdown temperatures and its PCIe link at most, from NVML, and its driver and CUDA versions, from NVML's system queries. Its PCIe link now comes from NVML too; on the RTX 2070 it is PCIe 1.0 while idle and 3.0 under load;
- the CPU's model and its cores, from sysinfo; the temperature's sensor is the one chosen above;
- a disk's model, from `/sys/class/block/<device>/device/model`, or its drive's for a partition, and whether it is an SSD and its filesystem, from sysinfo;
- a network adapter's link speed and duplex, from `/sys/class/net/<name>/speed` and `duplex`, its vendor and model, from udev's hardware database in `/run/udev/data/n<index>`, and its driver, from its `device/driver` link.

If no reading arrives for three intervals and a second, the window says measuring has stopped, and for how long, because the thread may have stopped or be stuck in a read, and the numbers would otherwise still look live.

A rate is computed from two readings, so it waits for the second. No rate is shown for an interval that spans a region with no readings, or in which a counter went backwards, as NVML's energy counter does after the driver reloads.

## GPU power

slm computes the GPU's power from NVML's energy counter, which counts the millijoules the card has used since the driver loaded. The power at a reading is the energy used since an earlier reading, divided by the time between them. The earlier reading is the newest one at least 2 s, less half the interval, before. The counter moves in steps of about 100 ms, so an average over 200 ms would be off by up to half its value. So the power chart lags a change of load by up to about 2 s. On a card without the energy counter, which is any card older than Volta, slm shows NVML's instantaneous reading instead.

On the RTX 2070 slm was measured on, the energy counter's 2 s averages were on average 12.6% below the mean of the instantaneous readings taken every 500 ms over the same 2 s, so slm's power reads lower than `nvidia-smi`'s `power.draw.instant`. Which of the two the card's power limit holds to is not known. slm does not read NVML's throttle reasons to tell when the card is held at its limit, because on this card the software power cap flag was set in one reading and clear in the next, half a second apart, under the same load.

"At or above threshold" is the share of the time the charts show during which the GPU's power was at or above its threshold. It counts each interval by its length in time, and judges it by the power at its later reading. An interval with no power counts on neither side. The threshold starts at 95% of the enforced limit, rounded to whole watts, and follows the limit until the person sets one. A GPU that reports no limit has no threshold until the person sets one.

## Checking a release by hand

The tests own the arithmetic. Before each release, on the maintainer's machine:

- Under GPU load, run `nvidia-smi --query-gpu=utilization.gpu,memory.used,enforced.power.limit,temperature.gpu,fan.speed,clocks.gr --format=csv -lms 500`. The GPU's load, VRAM, the power limit, the temperature, the first fan and the graphics clock agree with the window within one interval. `fan.speed` reports one value, so any further fan is checked against NVML's `Device::fan_speed` in a scratch program.
- Under a steady GPU load, the power shown stays within 20% of `nvidia-smi`'s `power.draw.instant`, which is as close as the two sources agree on the RTX 2070, and the power chart rises and falls with the load.
- RAM agrees with the "used" column of `free -b`.
- For each disk, the size and the space used (size less "Avail") that `df -B1` gives, in GiB, agree with the pie's tooltip within 0.05 GiB, and the pie's color agrees with the share `df` gives.
- `cargo test -- --ignored capture_scenarios` writes one image per scenario, and each shows the state it is named for.
- The window opens with the default settings the first time, and with the last settings and position after a normal close and a restart.
- The window shows its first state within a tenth of a second of opening, timed on a screen recording.
- A suspend shows on the charts as a region marked "Not measured".
- In a GNOME session on Wayland, the window has a frame with working buttons.

## What is not verified

These have tests with fake readings, but no machine to check them by hand:

- a machine with an NVIDIA GPU and no NVIDIA driver;
- a GPU that reports no power limit;
- a GPU older than Volta, without the energy counter.

## Statistics left for a later version

These were left out of the first version, and each could be read from the source named:

- PCIe throughput: NVML's `Device::pcie_throughput`.
- The video encoder's and decoder's utilization: NVML's `Device::encoder_utilization` and `Device::decoder_utilization`.
- The processes using the GPU: NVML's `Device::running_graphics_processes` and `Device::running_compute_processes`.
- Load average: `/proc/loadavg`.
- Uptime: `/proc/uptime`.
- The processes using the most CPU or memory: sysinfo's processes, from `/proc/<pid>/stat` and `/proc/<pid>/status`.
- The NVMe drive's temperature: the `nvme` sensor "Composite", through sysinfo's components.
- AMD GPUs: the `amdgpu` driver's sysfs files `gpu_busy_percent`, `mem_info_vram_used` and `mem_info_vram_total`, and its hwmon files `power1_average`, `power1_input`, `power1_cap`, `temp1_input`, `fan1_input` and `freq1_input`, which any user can read. A source for them goes beside `src/gpu/nvml.rs`.
- Intel GPUs: utilization only through the per-process DRM counters in `/proc/<pid>/fdinfo` (any `xe`, and `i915` from kernel 5.19, wrong under GuC submission), or through performance counters that need `CAP_PERFMON`. Most Intel integrated GPUs report no power limit and have no fan.

## Changing slm

| Module | Holds |
| --- | --- |
| `reading` | the plain data of one reading, and the reasons a value can be missing |
| `history` | the last hour of readings, and everything computed from them: regions with no readings, rates, GPU power, the time at or above the threshold, and the points to draw |
| `system` | CPU, memory, disks, network and CPU temperature |
| `gpu`, `gpu/nvml.rs` | the vendor-neutral GPU source, the search for NVIDIA GPUs on the PCI bus, and NVML |
| `sampler` | the sampling thread |
| `text` | every text the window shows or gives a screen reader, one slot each, as the copy step wrote them |
| `ui`, `ui/tests.rs` | the window, its tests, and the scenarios that render each of its states |

Four choices shape the code, and each has a reason not to undo it:

- slm reads the machine on a thread of its own, not in the window's loop. A minimized or hidden window may not get its loop woken, which would leave holes in the history nobody caused, and starting NVML or a slow read would freeze the window while it runs.
- The window is laid out for 350×200 (D43, D44): one line per measurement, in a grid of its name, its value and a small chart, then the temperatures, fans, clocks and swap as text that wraps. Each disk's space is a pie of the line's height beside its name, so it takes no line; its numbers are in its tooltip. The window's height is measured each frame from the top panel and what the central panel shows, and set when it changes by a pixel or more. The text is 11 px, the size of the mock-up the requester chose; at egui's 12.5 px only seven lines fit. A part of a whole writes its unit once, "5.9 of 8.0 GiB", which is what keeps the texts below the grid to four lines on a machine with one GPU. The settings are one menu behind one button, open until a click outside it, and scrolled, because egui cannot draw a menu beyond its window.
- The small charts are drawn by slm, not by egui_plot, whose axes and margins take more room than a 13 px line has. A chart of a percentage is scaled to 100%, the power chart to the card's limit, and a rate chart to its highest value in the span. Each chart has one unit, rather than one chart with every series as a percentage, because disk and network rates have no natural 100%. A chart has no legend: its tooltip names each series with its value then, and the threshold, and how long ago.
- Each logical CPU is a thin bar after the CPU's value, rather than a line of its own, so that D13's per-CPU load costs no line of the small window.

A missing value is never 0 in the history either: `reading::Missing` carries the reason, and each reason has its own tooltip. A failure is a sentence in `text`, never an NVML error's name.

Before each commit, run:

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

A test whose name begins with a decision's identifier, such as `d16_switching_history_keeps_readings`, protects that decision of `SPEC.md`.

`cargo test -- --ignored capture_scenarios` renders each state of the window into `target/captures/`, one PNG per scenario. It needs a graphics adapter, which CI runners do not have, so it is ignored by default. `cargo test -- --ignored capture_sizes` renders the running window at three window sizes into `target/captures/sizes/`, to check the layout by eye.
