# What good means, here

## Pointers

- **wgm** is at https://github.com/mirandadam/wgm/tree/v0.2 (commit `9a7eedd292cee8afc7c7c94a34a3ebb3258fc36f`).
- **Who may answer**: the default, whoever asks. Today that is the maintainer, Daniel Araújo Miranda, while working on a change, or on its issue.
- **Where things are written**:
  - The specification, with the product's vocabulary, is `SPEC.md` beside this file.
  - `docs/reference.md` is the as-built reference, for people who run slm and for people who change it. `README.md` is a short introduction with a screenshot, what slm shows and how to install it, as the maintainer asked on 2026-10-25. What only the next editor of a piece of code needs goes in a comment beside that code.
  - Frozen analyses, observations of use among them, go in `docs/analyses/`, one file each, dated in the name and in the text. There are none yet.
  - Insights drawn from observations go in `docs/insights.md`.
  - slm keeps no recordings or quotations of people, so there is no place for them.
  - The backlog is `BACKLOG.md`, beside this file. The repository is published, and the maintainer keeps the backlog there rather than in its issues, for now (2026-10-25).
  - Plans take the default, beside this file.
  - There are no agent instructions besides `CLAUDE.md`.
- **Good to use, here**: the standard's seven checks are written for a web browser and do not apply as written, because slm is a native desktop window. These carry over:
  - Every screen has its states: a missing value is never drawn as 0, and its tooltip gives its reason. Each panel has a state before its first measurement, one when its source is missing, and one when its source fails while slm runs.
  - A failure is a sentence in slm's words, never an error's name, saying what happened and what to do next.
  - Settings survive a restart, which stands in for "the address is the state".
  - One control per job: the settings are one menu behind one button, in which each setting is one row of choices, each GPU's threshold one field, and what is shown one list of boxes. Every control is at least 24 pixels tall, the WCAG 2.2 minimum for a pointer. Every chart's newest value is also in a text label beside it, which a screen reader reads. The small charts of the side window (D44) have no legend: a series is told apart by its color, and named, with its value, in the chart's tooltip and the value's.
  - "Recognition, not recall" has nothing to apply to, because slm asks for no decision. slm is for Linux desktops on X11 or Wayland. The machine it is built and measured on has an AMD Ryzen 5 2600X, an NVIDIA GeForce RTX 2070 and Ubuntu with GNOME on X11. The budget for waiting is measured by hand on that machine before each release. Its people already know GNOME System Monitor, and may not know `nvidia-smi`, as the maintainer said on 2026-10-24. The words slm uses are chosen by the copy step (D19 in `SPEC.md`), and its vocabulary is in `SPEC.md`.
- **How use is observed**: in the field form. The maintainer's own use is observed and marked as such, at each release. After publication, each issue about using slm is written as a reported observation.
- **Conventions in force**:
  - None of the four static checks exists, which is the default.
  - Commit messages are unprefixed, because the log is read by its one maintainer.
  - Texts are written in English.
  - The text slm's users read is written with the draft recipe `wgm/recipes/writing-text-user-sees.md`, which is not in wgm v0.2. It is a draft not yet published in wgm.
  - A plan is reviewed with `recipes/reviewing-a-change.md` in **one full round with one investigator**, carrying the deletion, adversarial and wording perspectives, instead of the default of two full rounds with up to three investigators each. The rounds on the fixes keep their defaults. The reason is that slm is a small single-maintainer tool, so a second full round or a second reader costs more than it would find.
- **What blocks a change**: nothing yet. There is no hook and no pipeline. `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` and `cargo test` are run by hand, and a GitHub Actions workflow runs the same three on every push and pull request; both only report.

## Standing reviews

| Review | Hunts for | Recipe and perspectives | Milestone |
| --- | --- | --- | --- |
| Release read | a release whose texts, code and claims no longer hold together | `recipes/reading-it-cold.md` | tagging a release |
