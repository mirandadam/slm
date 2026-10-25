# slm: specification

This holds the requester's decisions about what slm does, as a person sees it. Each has an identifier, which tests and code comments cite, the rule, and why. Daniel Araújo Miranda gave every decision here in the terminal session of 2026-10-24. How each is carried out is in the as-built reference and the code. An identifier is never reused: a decision that a later one replaced keeps its number and points to the later one.

## What slm is

- **D1. What it shows.** slm shows the current machine's main load statistics: CPU busy, memory used, and for each NVIDIA GPU how busy it is, its memory used and its power. Why: the request was "a tiny rust gui application to show the main load statistics of the current machine, including video card GPU/memory/power load."
- **D2. A dashboard window with charts**, not a compact always-on-top widget. Why: the requester chose "dashboard window with graphs". Its size is replaced by D43.
- **D43. A small side window.** slm is laid out for a small window beside the main work, about 350×200, where every panel's values and their recent history show without scrolling. Its charts and dense layout stay; what shrinks, and how, is Claude's choice, from mock-ups the requester compares first. This replaces the large first window of D41, and its own size is replaced by D49. Why: "this panel is intended to be use in a small side window while the main processing runs", and "smaller, like 350x200", on 2026-10-24.
- **D44. The side window's layout.** One line per measurement: its name, its value right after it, and a small chart of its recent history filling the rest of the line. Temperatures, fans and clocks share one line of text, and the settings sit behind one button. Why: the requester chose layout A of three mock-ups, and asked for less space between the text and the chart, on 2026-10-24. The texts of the temperatures, fans and clocks, with the threshold and swap, wrap over as many lines as they need: four on a machine with one GPU, as the requester accepted on 2026-10-24.
- **D45. Shorter names.** The names drop "busy" and "used": "GPU", "CPU", "VRAM", "RAM", "Swap". This replaces those words of the vocabulary. Why: "drop the 'busy' word", and "Also drop the 'used' word", on 2026-10-24.
- **D46. No chart for the time at or above the threshold.** It is a line of text, as before. Why: "drop the 'At or above' graph by default", on 2026-10-24. The mock-up had drawn one; slm never had it.
- **D47. The copy step again.** The texts of the side window are written by a new run of the copy step (D19), on the budget of D24. Why: "the 'copy' needs to improve", on 2026-10-24.
- **D48. How full each disk is.** Beside each disk's name, a very small pie shows the share of its space used: green below 70%, yellow from 70%, red from 90%. Its numbers, the space used of the size and the share, are in its tooltip and in what a screen reader reads, not in the texts below the grid. Why: the requester asked for it, by dictation, on 2026-10-24, and chose "as recommended" for the numbers' place (Q20).
- **D49. The window's size follows its contents.** The window is 350 px wide and exactly as tall as what it shows, which varies with the number of disks, network adapters and GPUs, and with the sentences of a failure. The person does not resize it, and only its position is kept across a restart. This replaces the size of D43 and the saved size of D14. Why: "it can be really small", and "the height would vary with the number of volumes, but the size stays fixed, except for that", by dictation on 2026-10-24, and "as recommended" (Q21).
- **D10. Name.** slm, for "system load monitor", for now. The repository will be public. Why: the requester's choice.
- **D11. License.** MIT OR Apache-2.0, copyright Daniel Araújo Miranda. Why: a public repository needs a license, and the requester accepted this one.
- **D12. How the work is held.** Every change follows wgm's standard, at the version the project's conventions name. Why: "My constraint is that you adhere to the principles of STANDARD.md."

## What it shows

- **D13. Statistics beyond D1.** slm also shows:
  - for each GPU: temperature, the speed of each fan, and the graphics clock against its maximum;
  - for the CPU: how busy each logical CPU is, the temperature, and the average clock;
  - for memory: swap used;
  - for each mounted disk: the space used against its size, and its read and write rates;
  - for each physical network adapter: its receive and send rates. Loopback and virtual adapters are left out.

  Why: the requester accepted this list. PCIe throughput, the video encoder and decoder, load average, uptime, the processes using the most CPU or memory, and the NVMe drive's temperature were recommended against and left out. Their sources are recorded in the as-built reference for a later version, as the requester asked.
- **D5. No list of the processes using the GPU.** Why: "not right now".
- **D6. Which disks.** Every real mounted disk is shown, with snap's loop devices and repeated mounts of one device left out, and the person does not pick. Why: "show everything as filtered for now."
- **D42. Snap's mounts.** The mount points snap makes for its packages, under `/snap/` and `/var/snap/`, are left out of a disk's mount points, as snap's loop devices are (D6). A disk mounted only there is left out. Why: on the requester's machine the disk's heading listed `/var/snap/firefox/common/host-hunspell` beside `/` and `/home`, which says nothing about the disk. Claude recommended leaving them out and the requester agreed, on 2026-10-24.
- **D7. No warning about low disk space.** Why: the requester said no. Replaced in part by D48: the pie's color is the only sign, with no message or notification, at fixed levels of 70% and 90%, which the person does not set. Why: "as recommended" (Q22), on 2026-10-24.
- **D50. What a pointer on a name tells.** Pointing at a name gives what helps tell what holds the work back, read without root:
  - the GPU's name: its architecture, VRAM, its PCIe link at most and now, its driver and CUDA versions;
  - the power: the limit, its default and the range it can be set in;
  - the GPU's temperature: where the card slows down and where it shuts down;
  - a network adapter: its link speed and duplex, also in MiB/s to compare with its rates, its model and its driver;
  - a disk: its model, SSD or HDD, and its filesystem;
  - the CPU: its model, and its cores and logical CPUs;
  - the CPU's temperature: the sensor it comes from.

  IP and MAC addresses and serial numbers are left out, because they are private and show in a shared screen; so is the CPU's maximum clock, which Linux gives below the boost. These texts are written by Claude from the vocabulary, not by the copy step of D19. Why: the requester asked for useful information on the items that had none, and accepted this list and this way of writing it, on 2026-10-25.
- **D8. Only mounted filesystems** appear, not disks that are not mounted. Why: the requester's choice.
- **D4. Other GPU vendors.** The first version reads NVIDIA GPUs only. The GPU code goes through one vendor-neutral interface, so that another vendor can be added without changing the rest. Why: "leave room for others". This reading of it is Claude's; the requester did not say more.

## How often, and how far back

- **D3. Measure every, and charts show last.** Both can be changed in the window. slm measures every 500 ms and charts the last 5 minutes by default. A change of the interval takes effect at the next measurement. Why: "Adjustable, default 500ms. History adjustable, default 5 minutes."
- **D16. What the span does.** slm keeps the last hour of measurements, whatever the interval. "Charts show last" only chooses how much of that hour is shown. Going from 15 minutes to 5 and back to 15 shows 15 minutes again at once. Why: "ideally store for 1 hour, the history option being for displaying. This needs to be smart about varying sampling intervals too."
- **D17. A suspend.** A time when slm measured nothing, such as a suspend, is shown on the charts as a marked region, "Not measured", at its real length. It looks different from the ordinary gap between two measurements at a long interval. No rate is computed across it. Why: "'no data' equivalent display, which is different from not having data between larger sampling intervals."
- **D41.** Replaced by D43. It decided that the first window shows every panel on a 1920×1080 screen, at 1400×900.
- **D14. Settings survive a restart.** "Measure every", "Charts show last", and the window's size and position are saved when slm closes and restored when it starts. Why: the requester accepted this. Its saved size is replaced by D49.

## Missing values and failures

- **D18. A value that is not there** is never shown as 0. It shows its own mark, and its tooltip says why it is missing. A GPU with no fan shows that mark on its fan line. The fan line can be hidden (D26). Why: "'no data' equivalent display, fan line is toggleable."
- **D20. An NVIDIA GPU that slm cannot read.** When the machine has an NVIDIA GPU and slm cannot start NVIDIA's driver library, the window shows a panel for that card. The panel says that an NVIDIA GPU is present and that slm cannot read it. It says why, and what to do next. When the cause is a missing driver, the next step is to install NVIDIA's own driver, because the open-source driver gives slm nothing to read. Why: the requester had no expectation, asked for a suggestion, and accepted this one.
- **D28. Where slm finds the card** (D20). slm finds an NVIDIA GPU whether or not any driver is bound to it. So the panel of D20 also appears when NVIDIA's kernel module failed to load. Why: the first place proposed lists a card only while some display driver is bound to it. That is not true in the very case D20 is for, because NVIDIA's driver package keeps the open-source driver from binding.

## The GPU's power

- **D9. Time at the power limit.** slm shows how much of the time the GPU's power was high. Why: the requester accepted this. The requester first chose to compare power with 95% of the limit the card reports, then replaced that comparison with D30.
- **D15.** Replaced by D30. It decided what a GPU that reports no power limit shows: the highest power seen, with a way to reset it.
- **D29. No measurement of the power limit.** slm does not try to tell when the card is held at its limit. The person sets a threshold instead. Why: "I don't want to do a measurement like that. Just add a configurable threshold to the user and report '% of time over threshold' (or something like it)".
- **D30. The threshold.** Each GPU has a power threshold in watts. Beside it, the line "Time at or above threshold" shows the share of the time the charts show during which the GPU's power was at or above the threshold. The threshold starts at 95% of the power limit the card enforces, rounded to whole watts, and follows that limit until the person sets one. Once set, it stays until the person sets it again; there is no way back to the starting value. A GPU that reports no limit has no threshold until the person sets one, and its line asks for one. The threshold is set in a number field beside the line, and survives a restart. Why: the requester agreed to this reading of D29. Since D44 and D47, the line reads "At or above threshold" and the field is in the settings menu.
- **D31. How the thresholds are kept** (D30). Each GPU's threshold is kept under the card's own unique identifier, only once the person has set it. It is kept while the card is absent, so it returns with the card. A card whose threshold was never set keeps nothing, so its starting value keeps following the limit. Why: the shape of saved settings is hard to change later. The requester accepted this.

## Choosing what is shown

- **D21. Hiding the fan line** (D18). It is not a checkbox on each GPU panel. It is a separate menu with checkboxes next to the names, which D26 describes. Why: "No, maybe a separate menu with checkboxes next to the widget names."
- **D26. The menu of what is shown.** One menu in the settings bar, apart from the panels, lists each panel by its name. Under each name it lists every line and chart of that panel, each with a checkbox. Every box starts checked, and the boxes survive a restart. Why: a menu with one entry per panel would not hide the fan line that D18 requires. The requester accepted the proposal.
- **D32. How the boxes are kept** (D26). Only the boxes the person has unchecked are kept, each under its panel and the name of its line or chart. A GPU is known by its unique identifier, as in D31, a disk by its device, and a network adapter by its name. The CPU and memory panels are known by their own names. A kept box stays while its device is absent, so the choice returns with the device. Why: the shape of saved settings is hard to change later. The requester accepted this.

## The words slm uses

- **D19. How the words are written.** Every text the window shows is written by the copy step that the project's conventions name. Its writers work from notes on what each place does, and never see the old text. Why: the requester asked for it, by name, instead of approving words proposed in a plan.
- **D27. Which texts.** The copy step writes every text the window shows or gives a screen reader: labels, units, tooltips, the sentence for each state, the menu entries, the settings bar and the window's title. The as-built reference is documentation, and is out of its scope. Why: the requester accepted this.
- **D22. English only, for now.** Why: "English only for now."
- **D23. Who reads slm.** A person who knows roughly what a CPU, a GPU, VRAM, swap and a power limit are, and has used GNOME System Monitor. They have not necessarily used `nvidia-smi`, and have not read slm's documentation. Why: "Don't assume they have used nvidia-smi. System Monitor is ok. The rest is ok."
- **D24. Budget.** The copy step runs one explainer and two writer runs per language, on the model the session runs on, with no pilot. Why: slm's interface is one screen, and the requester agreed.
- **D25. Writers invent no details.** A writer may not add a number, cause, remedy, product name or command the notes do not give. Each such detail is a finding instead. Why: "No. That part exists because I used that recipe to write a game."

These words were chosen by the copy step of 2026-10-24. The requester decided the ones the two writer runs did not agree on, each from the two runs' best candidates, as "All as recommended":

- **D33.** The window's title is "slm", not "System load" or "Load Monitor". Why: it is the name the app is known by.
- **D34.** The span's label is "Charts show last", not "Show last". Why: the numbers are always live, and only the charts and the share follow it.
- **D35.** The button of the menu of D26 is "Choose lines and charts…", not "Show…". Why: it names what the menu holds, and does not repeat D34's verb. Since D44 the boxes are inside the one menu of the settings, under a heading, and there is no button of their own to name.
- **D36.** The panels are "Memory" and "Disks", not "Memory and swap" and "Mounted disks". Why: the lines say RAM and swap, and the empty state says "No mounted disks found".
- **D37.** The mark of a missing value (D18) is "—", not "n/a". Why: "n/a" reads as "not available", which is false for a value that is coming or that failed once.
- **D38.** A part of a whole reads "{part} of {whole}", not "{part} / {whole}". Why: a screen reader says "/" as "slash".
- **D39.** Rates are in "KiB/s" and "MiB/s", not the "K/s" and "M/s" both runs chose to fit a narrow chart axis. Every chart's value axis is widened to fit the longest label instead. Why: the rest of the window writes GiB, and "K/s" does not say bytes. The cost is that each chart is narrower, measured at about 28 px. Since D44 the charts are small and have no value axis; the units stay, in the values and the charts' tooltips.
- **D40.** A chart is named in the menu as "… chart", such as "Power chart", not "Chart: power". Why: it reads as a name beside the names of the lines.

### Vocabulary

Words to use for slm's concepts, and words to avoid. "Both runs" means both writer runs chose it as their best candidate, independently.

| Concept | Use | Avoid | Rests on |
| --- | --- | --- | --- |
| the program | slm | System load, Load Monitor | D33 |
| how often slm measures | Measure every | refresh, interval | both runs |
| how far back the charts reach | Charts show last | history, span | D34 |
| the menu of what is shown | Choose lines and charts (a heading since D44) | Show… | D35 |
| the panels | CPU, Memory, Disks, Network | Memory and swap, Mounted disks | both runs, D36 |
| one sample of the machine | measurement | reading, which names a disk's read rate | both runs |
| the state before any measurement | Measuring… | | both runs |
| how busy a processor is | GPU, CPU | utilization, usage, load, busy | D45 |
| one hardware thread | logical CPU, "CPU {n}" | core | both runs |
| the GPU's memory | VRAM | VRAM used | D45 |
| working memory and swap | RAM, Swap | RAM used, Swap used | D45 |
| the cap the card enforces | Power limit | cap, power.limit | both runs |
| the person's level | Threshold | level | both runs |
| the share at or above it | At or above threshold | over threshold, Time at or above threshold | the copy step of D47 |
| a span with no measurement | Not measured | no data | both runs |
| a time on a chart | "{time} ago" | | both runs |
| a missing value | — | n/a, 0 | D37 |
| a part of a whole | "{part} of {whole}" | / | D38 |
| a disk's activity | Reading, Writing | | both runs |
| a network adapter's traffic | Receiving, Sending | download, upload | both runs |
| units | %, W, GiB, °C, MHz, ms, s, min | | both runs |
| rate units | KiB/s, MiB/s | K/s, M/s | D39 |
| a chart in the menu | "… chart" | "Chart: …" | D40 |
| the button of the settings and the menu of what is shown | Settings | Options | the requester, 2026-10-24 |
| temperatures below the grid | GPU temp, CPU temp | Temperature | both runs, 2026-10-24 |
| the CPU's average clock | CPU clock | Clock (average) | both runs, 2026-10-24 |
| no swap set up | none | | both runs, 2026-10-24 |
