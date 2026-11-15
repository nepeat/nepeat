# ai_re — Hardware Reverse Engineering

Rules and conventions for hardware RE work in this project. These OVERRIDE default behavior for anything under `ai_re/`.

## Dev environment

- **All work happens inside the nix devshell.** Enter it with `nix develop` (or via `direnv` if `.envrc` is present) before running any tooling.
- The devshell is defined by `ai_re/flake.nix`. The agent is **allowed and expected to edit `flake.nix`** to add tools as needs arise (serial tools, flashers, logic analyzer software, disassemblers, SDR utils, etc.). Evolve it deliberately — add a package when a task needs it, note why in the commit.
- Prefer packages from nixpkgs. If something isn't packaged, document the manual install in the relevant device subdir rather than polluting the shell silently.
- After editing `flake.nix`, re-enter the shell (`exit` then `nix develop`, or `direnv reload`) so changes take effect.

## Shared Nix: `ai_re/nix/`

Everything Nix that *runs on or is built for a device* (kernels, bootloaders, drivers, NixOS modules, system configs, images) lives in `ai_re/nix/`, **not** in the device subdir. Read `nix/README.md` first; it lists what exists and how to add a board.

- Layout: `nix/packages/` (derivations), `nix/modules/{profiles,platforms,<function>,boards}/` (NixOS modules), `nix/configs/` (complete systems). The devshell stays in the root `flake.nix`.
- **Reuse before writing.** Several devices share a platform (e.g. the AS7760 and other NXP T2080 / K2-style cards). Look for an existing profile, platform or function module (thermal, switch chip, …) and import it. Put new work at the most general layer where it is true; when a second device needs something a board module has, move it up rather than copying.
- Patches are shared too: keep one ordered series per upstream (U-Boot, kernel) with a per-patch scope (platform-generic vs board-only), so a sibling board can take the generic ones.
- Build and deploy with `nix build path:./nix#<name>` (works before the files are committed) and the device's `deploy.sh`. Keep build logs and `result-*` links out of `nix/` (use the device's `nixos-build/`).
- New targets get the shared base: `profile-appliance` (small image, key-only sshd, probing tools, htop + fastfetch, flakes enabled). Check that a package cross-builds for the target before adding it to a shared profile.
- Refactors must be provably neutral: same `.drvPath` or output hash, or a `nix store diff-closures` that shows only the intended change. Record that check in the device's PROGRESS entry.
- `nix/` never reads `devices/` (dumps, keys and private inventories live there).

## Serial console workflow

- Interact with serial devices using **tmux + a serial console application** (e.g. `picocom`, `minicom`, or `tio`). `tio` is the default — it handles reconnects and logging cleanly.
- Run the serial session inside a **named tmux session** so it survives across agent tool calls and can be reattached/inspected:
  - Start: `tmux new-session -d -s serial 'tio /dev/tty.usbserial-XXXX -b 115200'`
  - Send input: `tmux send-keys -t serial '<command>' Enter`
  - Read output: `tmux capture-pane -t serial -p`
- Never assume the baud rate — record the confirmed baud/parity/framing in the device's subdir notes once known.
- Log serial sessions to a file in the device subdir when capturing boot logs or dumps (`tio --log-file …` or tmux `pipe-pane`). **Save captures as `.txt`, not `.log`** — the monorepo's root `.gitignore` excludes `*.log`, so a `.log` capture would be silently dropped from commits.
- On macOS, serial adapters appear as `/dev/tty.usbserial-*` or `/dev/tty.usbmodem*`. List with `ls /dev/tty.*`.

## Notes & organization

- **Every device or subproject gets its own subdirectory** under `ai_re/devices/<device-name>/`. **All notes for a project live inside that project's subdir** — nothing device-specific goes in the project root. (Its Nix code is the one exception: `ai_re/nix/`, see above.)
- Each device subdir must contain a **`PROGRESS.md`** — the entry point and running journal (dated entries, newest first). It captures:
  - Device identity: make, model, part numbers, chip markings.
  - Interfaces found: serial pinout + baud, JTAG/SWD, SPI flash, USB, etc.
  - What's been tried and what worked / failed (dated entries).
  - Dumps, captures, and firmware live in the subdir too — but keep large binaries out of git unless intentional.
- Keep general/reusable RE technique notes at the project root; keep everything device-specific in the device subdir. Don't mix.
- **Porting a switch or similar appliance?** Follow `switch-porting-runbook.md` (phased checklist with gates, gotchas, and the decisions that need the owner). Tick it per device in that device's PROGRESS.md, and add new gotchas to the runbook when you hit them.

## Keeping docs current and lean

Docs are for the next reader (human or agent) who has ten minutes. Update them **in the same turn as the change**, and make them shorter whenever you can.

- **`PROGRESS.md` shape:** a `## Status` block at the top (what the device runs now, how to reach it, how to recover it, what is open — rewritten in place, about 15 lines), then dated entries, newest first. An entry is the outcome in a few bullets plus links; not a narration of the session.
- **One fact, one home.** Detail goes in a topic file (`u-boot-port.md`, `thermal.md`, …); PROGRESS links to it. Don't restate the same procedure in three files.
- **Edit, don't append.** When something stops being true, fix or delete the old sentence where it stands (keep the date of the change in the journal entry). A doc that says "now X" above "currently Y" is a bug.
- **Archive, don't accumulate.** When PROGRESS passes roughly 300 lines, move the oldest entries verbatim to `HISTORY.md` and leave a one-line pointer. A handoff note (`HANDOFF.md`), if a device has one, describes only the present and is rewritten, never stacked.
- **Evidence by reference.** Save transcripts, logs and dumps as files and link them with a hash or key line; don't paste them into markdown.
- After moving or renaming files, grep the device's `*.md` and scripts for the old paths and fix them.
- Lean never means hiding things: failures, dead ends that would otherwise be retried, and anything risky or unverified stay in, stated plainly.

## Safety & discipline

- Confirm voltage levels before connecting anything (3.3V vs 5V logic) — record the assumption in NOTES before probing.
- Prefer read-only / non-destructive interaction first (boot logs, dumps) before writing to flash or fusing anything irreversible.
- This is authorized RE on the user's own hardware. Keep records honest: log what actually happened, including failures.
