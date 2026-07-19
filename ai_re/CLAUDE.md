# ai_re — Hardware Reverse Engineering

Rules and conventions for hardware RE work in this project. These OVERRIDE default behavior for anything under `ai_re/`.

## Dev environment

- **All work happens inside the nix devshell.** Enter it with `nix develop` (or via `direnv` if `.envrc` is present) before running any tooling.
- The devshell is defined by `ai_re/flake.nix`. The agent is **allowed and expected to edit `flake.nix`** to add tools as needs arise (serial tools, flashers, logic analyzer software, disassemblers, SDR utils, etc.). Evolve it deliberately — add a package when a task needs it, note why in the commit.
- Prefer packages from nixpkgs. If something isn't packaged, document the manual install in the relevant device subdir rather than polluting the shell silently.
- After editing `flake.nix`, re-enter the shell (`exit` then `nix develop`, or `direnv reload`) so changes take effect.

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

- **Every device or subproject gets its own subdirectory** under `ai_re/devices/<device-name>/`. **All notes for a project live inside that project's subdir** — nothing device-specific goes in the project root.
- Each device subdir must contain a **`PROGRESS.md`** — the running log / journal for that project (dated entries, newest first). It captures:
  - Device identity: make, model, part numbers, chip markings.
  - Interfaces found: serial pinout + baud, JTAG/SWD, SPI flash, USB, etc.
  - What's been tried and what worked / failed (dated entries).
  - Dumps, captures, and firmware live in the subdir too — but keep large binaries out of git unless intentional.
- **`PROGRESS.md` is the entry point.** When it grows too large, break detail out into focused sibling files in the same subdir (e.g. `uart.md`, `bootloader.md`, `flash-dump.md`, `firmware-build.md`) and keep `PROGRESS.md` as a short index + latest-status summary linking to them.
- Keep general/reusable RE technique notes at the project root; keep everything device-specific in the device subdir. Don't mix.

## Safety & discipline

- Confirm voltage levels before connecting anything (3.3V vs 5V logic) — record the assumption in NOTES before probing.
- Prefer read-only / non-destructive interaction first (boot logs, dumps) before writing to flash or fusing anything irreversible.
- This is authorized RE on the user's own hardware. Keep records honest: log what actually happened, including failures.
