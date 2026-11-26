# RE toolkit helpers

## lab-power: lab power feeds

`lab-power` reads or switches a lab power feed: a Home Assistant smart plug or the
NetBotz 250's switched outlet. It is the Rust CLI in
[rust/lab-power](../../rust/lab-power/) (targets are data: `targets.json` there), on
PATH in the devshell (reload direnv after pulling changes).

Its environment lives in the untracked `.envrc.local`, which `.envrc` sources through
direnv (plain `nix develop` does not load it):

- `HA_BASE` (the server's `http://` URL without `/api`; https is not supported) and
  `HA_KEY` (a long-lived access token): the Home Assistant targets.
- `LAB_POWER_NETBOTZ_CREDENTIALS`, `LAB_POWER_NETBOTZ_KNOWN_HOSTS`: absolute paths of
  the NetBotz credentials JSON (`username`, `password`) and its pinned `known_hosts`
  (both in the git-ignored `devices/apc-general/snapshots/lab/`). Files that group or
  others can read are refused.

Never commit these or put a secret in an argument. Every call names its target: there is
no default, so `lab-power off` alone is refused before anything is contacted.

| target | backend | feeds | power meter |
| --- | --- | --- | --- |
| `1` | HA `switch.server_room_lab_plug` (Onvis S4) | CS8210-32X, its PSU not on the NetBotz (PSU 2 per `psushow`, 2026-11-26) | none |
| `2` | HA `switch.server_room_lab_plug_2` (Onvis S4) | `dmc-12-48r-as0` console server | none |
| `cs8210-psu1` | NetBotz 250 `10.60.30.45`, its one switched outlet (`nboutlet`) | CS8210-32X PSU 1 | none |
| `cs8210` (device) | all feeds: `1` + `cs8210-psu1` | the CS8210-32X | none |

```sh
lab-power 2 status
lab-power 2 off; lab-power 2 on
lab-power 1 cycle --off-time 15
lab-power cs8210-psu1 status --json
lab-power cs8210-psu1 normal-state on   # NetBotz only: the state after the NetBotz itself restarts (read back)
lab-power cs8210 status          # every feed
lab-power cs8210 cycle           # true cold cycle: all feeds off, wait, all on
lab-power cs8210 power           # watts where a feed has a meter
lab-power --list
```

- `status` only reads. `on`/`off` read first, switch, then read back until the feed
  reports the new state (`--wait`, default 5 s). Exit 1 on failure, 2 on usage. HA's
  `unknown`/`unavailable` are shown by `status` and refused by `on`/`off`. A reported
  state is not an independent measurement of mains power.
- `cycle`: off (verified), `--off-time` (default the target's `cycle_off_seconds`, 10 s),
  on (verified). `off` and `cycle` print what the target feeds first (stderr); there is
  no prompt.
- Devices act on all their feeds. Every feed is read first; if one cannot be read,
  nothing is switched. If a feed does not turn off during `cycle`, the cycle is aborted,
  every feed is switched back on, and the error names the feed.
- `power [--watch S [--count N]]`: watts from a target's HA `power_sensor` (W or kW),
  else `no power meter on this target`. None of today's targets has one: the Onvis S4
  plugs expose only switch/firmware/identify/power-on-behavior entities in HA, and the
  NetBotz outlet does not measure current. The PSUs' PMBus telemetry is the CS8210's own
  (`psushow`; devices/cs8210-32x).
- `--json`: one object per call (HA targets carry the entity as `home_assistant`).
  `--timeout` is per HTTP request or SSH prompt (default 10 s).
- NetBotz: the system `ssh -tt` with `-F /dev/null`, password auth only (the password
  reaches ssh through `SSH_ASKPASS`, which is lab-power itself reading the credentials
  file), the pinned `known_hosts` with `StrictHostKeyChecking=yes`, and
  `HostKeyAlgorithms=+ssh-rsa` (the AP9538 NMC2 offers only `ssh-rsa`). One session per
  read or switch (about 2.5 s), always closed with `bye`. The NMC allows few sessions,
  so do not run several at once. Errors say unreachable, authentication failed, host key
  changed or not verified, session limit, or unexpected CLI output; ssh's own messages
  are never printed (they carry key fingerprints). `nboutlet -ns` (the normal state,
  now `Off`) is never sent.

To add a target, add it to `targets.json` (backends `home-assistant`, `netbotz`) and to
this table. API reference: [Home Assistant REST API](https://developers.home-assistant.io/docs/api/rest/).

Tests (no hardware, no real dumps):

```sh
just test-tools    # pytest for ai-re-images, then nix build .#lab-power: fmt, clippy and its
                   # tests (fake HA server, fake ssh replaying the recorded nboutlet transcript)
```

`just test` also test-builds everything under `nix/` (U-Boot variants,
packages, systems); the tiers are in [nix/README.md](../nix/README.md#tests-just-test).

## ai-re-images: image catalog

Lists, tags, verifies and inspects every dump under
`devices/*/{dumps,dump,images,firmware,captures}`, across devices and units,
without changing them. Images are never written, moved or mounted.
Files with `private` in their path are skipped, so their contents never
reach the output or a sidecar.

- **Recorded facts stay with the device.** The tool reads the files already
  there (`manifest.json[l]`, acquisition/verification JSON, `SHA256SUMS`,
  `RAW-SHA256SUMS`, `*.sha256`, `device-sha256.txt`) and never rewrites them.
- **Its own additions** (tags, notes, `set` overrides of kind/unit/version,
  `inspect` results) go in one tracked sidecar per device:
  `devices/<device>/images.jsonl`. There is one row per image, keyed by
  device-relative path plus content sha256. If a file is moved, the sha256
  re-attaches its row. The sidecar sits at device level, not in
  `dumps/<unit>/`, so acquisition dirs stay pure tool output (`acquire.py`
  writes there). It also covers devices with no unit layout. It is public:
  `inspect` stores only safe fields (ONIE serial, MAC and service tag, U-Boot
  env values, and filesystem UUIDs are withheld).
- **Identity** = sha256 of the stored file (the `.zst` when the raw file was
  removed), plus the raw-content sha256 when one is recorded. `dupes` groups by
  content. Hashes of untracked files are cached in `~/.cache/ai_re-images/`.

```sh
ai-re-images scan [--hash]                  # per device: ok / zst-only / missing / untracked; --hash names untracked files
ai-re-images list --device dmc-12-48r-as0 --sort version
ai-re-images list --kind spi-nor --tag original-state --json
ai-re-images show dmc-12-48r-as0:dumps/u01/sda.img.zst     # or a path, or a sha256 prefix
ai-re-images tag REF factory os-1.0.457.0; ai-re-images untag REF factory
ai-re-images note REF "read via /dev/mtd0 from the rdinit shell"
ai-re-images set REF --kind spi-nor --version 2.60.4         # '' clears
ai-re-images verify --device dmc-12-48r-as0 [--raw]          # --raw also hashes the decompressed stream
ai-re-images inspect [REF...] [--deep] [--no-store]
ai-re-images dupes
ai-re-images export-qcow2 REF /scratch/u01-sda.qcow2
```

Kinds come from the name, then `inspect`, then a `set` override: `disk`,
`partition`, `flash` (use `set --kind spi-nor` once that's known), `env`,
`eeprom`, `dtb`, `fit`, `kernel`, `initramfs`, `rootfs`, `firmware`, `rom`,
`config`, `other`. Unit = `dumps/<uNN>/`. Filters: `--device --unit --kind
--tag --text --status`. Every command takes `--json`.

`verify` re-hashes against every recorded hash (host and on-device). A dump whose
raw file was removed after compression reports `raw removed, .zst verified`.
With `--raw` it reports `raw removed, content verified`.
`inspect` reads headers only: partition table (`sfdisk --json`, from a sparse
copy of the stream head for `.zst`) and the filesystem at each partition
start. Partitions more than 1 GiB into a compressed stream need `--deep`.
It also reads U-Boot legacy/FIT (`dumpimage -l`), DTB model, ONIE `TlvInfo`,
CRC-valid U-Boot env (either byte order), ELF and cpio. For images of 256 MiB
or less it adds a map of embedded headers and the 0xff/0x00 fill. Logical
partitions in a compressed stream are not read.

`export-qcow2` writes a zstd-cluster qcow2 (`qemu-img convert -c -O qcow2 -o
compression_type=zstd -m 4`). For a `.zst` it first decompresses to a sparse
temp file in `--scratch` (default: next to OUT), checks the recorded raw
sha256 on the way, then deletes the temp. It refuses an OUT inside a
device image dir unless `--next-to-original` is passed. It only prints the
attach commands (`qemu-nbd --read-only`, or `nbdkit --readonly
--filter=qcow2dec file X`, then `mount -o ro,noload`), because they need root.

Exit codes: 0 ok; 1 mismatch, conflict or failure; 2 usage or unknown REF;
3 `verify` found missing files but no mismatch. Decompression is
single-threaded and streamed (1 MiB chunks), so memory stays flat even for a
32 GB image.

## ghidra

Ghidra (nixpkgs `ghidra`, 12.1.2) and a headless MCP server for it
(`pyghidra-mcp` 0.3.0, [upstream](https://github.com/clearbluejar/pyghidra-mcp),
Apache-2.0) are in the devshell. Both are pinned by `flake.lock` / `flake.nix`;
nothing is pip-installed.

**Project:** `devices/delta-tn48m/ghidra/tn48m.gpr` holds `ap806-bootrom.bin`
(raw binary, `AARCH64:LE:64:v8A`, base `0xFFFF0000`, auto-analysed, plus two
seeded functions `reset_vector` @ `0xFFFF0000` and `bootrom_init` @
`0xFFFF040C`). The directory is git-ignored (`devices/*/ghidra/`, `*.gpr`,
`*.rep`) because a Ghidra project contains the firmware bytes and the monorepo is
public. Rebuild it with the two commands under "Add another binary".

**GUI:** `ghidra` inside the devshell, then File > Open Project >
`devices/delta-tn48m/ghidra/tn48m.gpr`. A Ghidra project can be open in only one
process: close the GUI (or the Claude Code session) before using the other, or
you get a lock error.

**MCP server:** registered in `ai_re/.mcp.json` as `ghidra-tn48m`. Claude Code
starts it over stdio; it needs no direnv and no open GUI. The command is
`nix run <ai_re>#pyghidra-mcp -- --transport stdio --no-symbols --project-path
.../tn48m.gpr`. It is deliberately `nix run`, not `nix develop -c`, because the
devshell `shellHook` echoes to stdout and would corrupt the MCP stream. The
`--no-symbols` flag turns off Ghidra's PDB symbol-server lookup. First launch
evaluates/builds the package (a few seconds if cached), later ones take ~5-10 s
to start. Approve the project-scoped server once in Claude Code (`/mcp`).

Tools: `search_symbols_by_name` (use `functions_only` + regex, e.g. `.` to list
functions), `decompile_function`, `disassemble`, `read_bytes`, `search_strings`,
`list_xrefs`, `list_exports`, `list_imports`, `gen_callgraph`, `rename_function`,
`rename_variable`, `set_variable_type`, `set_function_prototype`, `set_comment`,
`save` (renames/comments are only persisted after `save`), plus
`import_binary` (imports any local path into the project; it does not take a
base address, so use the script below for raw firmware). Addresses are hex,
e.g. `0xFFFF040C`. Notes:

- `search_code` (semantic search) is switched off in our build: upstream fills a
  chromadb index at startup, and chromadb's default embedder downloads an ONNX
  model from chroma-onnx-models.s3.amazonaws.com the first time. We patch the
  default to `code=False` (see `postPatch` in `flake.nix`) so the server never
  needs the network. Use `search_symbols_by_name` / `search_strings` instead.
- Clearing a comment with an empty string leaves an empty `/*  */` plate.
- On shutdown the server logs `ReadOnlyException: File is read-only` while
  saving; changes made through the `save` tool are persisted anyway.

**Add another binary** (raw blob; name the project dir/file like the TN48M one):

```sh
nix develop -c tools/ghidra/import-raw.sh <binary> devices/<dev>/ghidra <project> <base-addr> [language-id]
nix develop -c ghidra-py tools/ghidra/seed_functions.py devices/<dev>/ghidra/<project>.gpr <binary-name> name=0xADDR ...
```

then add another entry to `.mcp.json` pointing `--project-path` at the new
`.gpr`, or reuse the same project by importing into it (the importer adds a
program to an existing project). ELF/PE/etc. need no `-loader BinaryLoader`:
`import_binary` or `ghidra-analyzeHeadless` auto-detect them. Never commit the
project directory.

## ai-re-metrics: lab metrics (VictoriaMetrics)

Single-node VictoriaMetrics on the workstation, run as a **user** systemd
service (`ai-re-victoriametrics`, listens on 127.0.0.1:8428, 180 d retention).
Data, scrape config and targets live in `~/.local/state/ai_re-metrics/`
(override with `AI_RE_METRICS_DIR`), never in the repo, because targets carry
addresses.

```sh
ai-re-metrics start                       # idempotent; after a reboot run it again
ai-re-metrics target add as7760 <switch-ip>:9100 --label device=edge-core-7760-32x
ai-re-metrics target ls | target rm as7760
ai-re-metrics status                      # service + per-target health/last error
ai-re-metrics query 'switch_optic_temperature_celsius'
ai-re-metrics range 'max(node_hwmon_temp_celsius)' --since 6h --step 5m   # min/avg/max
```

Agents: add a target when you bring up an exporter on a device and remove it
when the device goes away; the NAME becomes the `job` label, so use the device
directory name. Targets are picked up within about 10 s, no restart. The web
UI (vmui) is at http://127.0.0.1:8428/vmui.

### Grafana

`ai-re-metrics start` also starts Grafana (nixpkgs, user unit `ai-re-grafana`;
`stop`/`restart`/`status` cover both). Open **http://<workstation>:3000**:
anyone on the LAN can view (anonymous Viewer); sign in as `admin` to
administer. The admin password is generated on first start and kept in
`~/.local/state/ai_re-metrics/grafana/admin-password` (mode 0600, never in
the repo). Grafana's config, database and logs are generated under that
`grafana/` directory on every start; nothing there needs editing. Every outbound
call (usage stats, update/plugin checks, news, gravatar, external snapshots,
plugin installs) is switched off. The LAN needs 3000/tcp open in the
workstation's firewall (or, tailnet-only and without opening it:
`sudo tailscale serve --bg --https=443 http://127.0.0.1:3000`; undo with
`sudo tailscale serve --https=443 off`). The first start takes about two minutes (database
migrations).

Dashboards are the JSON files in `tools/grafana/dashboards/`, provisioned
read-only into the `ai_re` folder (`allowUiUpdates: false`: the repo is the
source of truth, the UI cannot save them). Grafana re-reads the directory
every 30 s, so editing a file needs no restart. To add or change one: build it
in the UI ("Save as copy" or Export > JSON), write the JSON to
`tools/grafana/dashboards/<name>.json` with a fixed `uid`, tag `ai_re` and the
datasource `{"type": "prometheus", "uid": "victoriametrics"}`. Keep them
public: no IPs or hostnames, select devices with the `$job`/`$instance`
variables. Check it with `curl -s localhost:3000/api/dashboards/uid/<uid>`.
The tool finds the directory from the working directory (or
`AI_RE_GRAFANA_DASHBOARDS`), else reuses the path of the last start.

| uid | dashboard |
| --- | --- |
| `ai-re-lab-overview` | home page: one row per job (up, uptime, load, CPU, memory, root fs, NIC, max board temp, fan duty, optics, data age, failed units) |
| `ai-re-switch-thermals` | board/optic temperatures, temperature trend, fan duty, rotors, faults, fan-control state (as7760-thermal, fand) |
| `ai-re-switch-optics` | module inventory with DDM, RX power (dBm) and TX bias per lane |
| `ai-re-node` | node_exporter host view: CPU, load, memory, PSI, disk, filesystems, network, systemd, textfile collector |
| `ai-re-sonic` | SONiC switch (sonic-metrics): ports with rates/errors, PortChannels, BGP and BFD state timelines, LLDP, transceivers, ACL/CoPP, routes, config conflicts, unit restarts; linked from the lab overview's SONiC panel |
| `ai-re-dmc12` | DMC-12 console server: fan rpm/duty, CPLD watchdog failsafe, board temperatures, PSUs, CPLD alarms, serial ports present/in use, port map |

The node_exporter interface/disk label is `exported_device` because targets
carry `device=<name>` (the target's label wins).
