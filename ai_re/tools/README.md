# RE toolkit helpers

`lab-power` reads or switches a Home Assistant power outlet. Enter the nix
devshell (reload direnv after pulling changes) to put it on PATH. The helper
uses Python's standard library; no pip install is needed.

Keep `HA_BASE` (the server URL, without `/api`) and `HA_KEY` (a long-lived access
token) in your local environment: put them in the untracked `.envrc.local`,
which `.envrc` sources through direnv. Plain
`nix develop` does not load `.envrc` exports; use direnv or export them yourself.
Never commit tokens or paste them into CLI arguments.

```sh
lab-power status
lab-power off
lab-power on
lab-power status --json
lab-power status --entity switch.another_outlet
lab-power on --entity switch.another_outlet
```

The default is `switch.server_room_lab_plug`. Set `HA_ENTITY` to change the
default, or use `--entity` for a single command (takes precedence over the
environment). Entity IDs must be in the `switch` domain.

`status` only reads. `on` and `off` first check the entity, call the switch
service, then poll until HA reports the requested state. Failure returns a
nonzero exit code; `unknown` and `unavailable` are shown as-is by `status`.
`--timeout` controls each request's timeout (default 10 seconds); `--wait`
controls the polling window (default 5 seconds, plus any in-flight request).
HA's reported state is not an independent measurement of physical mains power.

API reference: [Home Assistant REST API](https://developers.home-assistant.io/docs/api/rest/).

Run the isolated API tests without touching hardware:

```sh
python3 -m unittest discover -s tools/tests -v
```

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
workstation's firewall. The first start takes about two minutes (database
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

The node_exporter interface/disk label is `exported_device` because targets
carry `device=<name>` (the target's label wins).
