# Backlog

What is still open and actionable.

- **Check the first release by hand.** Run the checks under "Checking a release by hand" in `docs/reference.md`. These have not been done yet: the GPU values under a steady load, a suspend, a restart after a normal close, the window on Wayland, and the time to the first state.
- **Observe the first release in use.** Observe the maintainer's own use, in the field form of wgm's `recipes/observing-use.md`, and mark it as the maintainer's. The guess slm was built for is that someone running a game or a GPU job wants to see, without a terminal, whether the GPU or the CPU holds the work back, and whether the GPU is held at its power limit. The guess is Claude's, from the conversation of 2026-10-24. Look for the person opening a terminal or GNOME System Monitor to check a number while slm is open. Also look for them asking what a number or the threshold line means, and for them looking for something slm does not show.
