# 2026-11-10 - Credo SAI 1.2.12 (x86_64): static review

Static review only. Nothing was executed (no binary run natively or under
emulation) and nothing touched the switch. Tools: `file`, `readelf`, `objdump`,
`nm`, `strings`, `llvm-dwarfdump`/`llvm-nm` (nix), `gh api`, and small Python
scripts over `objdump` output. The `libsai.so.1` and `libSerdes_owl*.so`
DWARF was used to recover structs, enums and signatures.

Legend: **V** = seen directly (disassembly, DWARF, strings, header); **I** =
inferred (reasoning from V facts, not directly observed).

Scope, with sha256 (`dl/` debs are byte-identical to the `x/` extraction; the
debs' `md5sums` all verify against the extracted files, V):

| file | sha256 |
| --- | --- |
| `dl/libsaicredo_1.2.12_amd64.deb` | `ca66a1ef34005fad784b30cdb91aeb671caf2cf361349b805da4294b2e35b5ca` |
| `dl/libsaicredo-owl_1.2.12_amd64.deb` | `9135103963f31a564812b029e6fe2617a6300423ca6c2a6a184d5691ff2e765d` |
| `libsai.so.1` (`libsai.so` is a symlink) | `7fae4b878e0eaec5f9027964cd86eb33d9aa5c68a2a2d342c9949f3d6ed1d657` |
| `libcredo.so` | `be74f18754bc6e6e1493359f663b2f9e932366010ce925b9dead0a53969d8763` |
| `libcrlua.so` | `f0675c7964342bbf987cd5bf6990c190bd6056b09d0a5790c0a1896e9c693906` |
| `libdriver_164_u.so` | `3fbaafca982659bf81a7c4bde5ff78a2f559954cc86505db7a424ff1a4d70a6b` |
| `crcli` | `a7638265e0e89299edab91178dccc7892564d51a15a4bbbfc0ebc7cace9e987e` |
| `libowl.so` | `99b1a2f20b3b3b0b428429bb79eb41c72fc49c25087b2aa287744c3add60f6fa` |
| `libSerdes_owl400.so` | `6a2196651a0bc21a1ba14ff04230e818150f346bed8a83c8ce6ca98665bec275` |
| `libSerdes_owl800.so` | `01109eeb8bcd66545852952095756031f2ac7a7dda3ddd3fa5c2e62f58fadd66` |
| `B52b0.lz4.fw.1.94.11.bin` (`firmware/owl` symlink -> it) | `92ba4433a58dd84bfbdcb66cc4dd6753b2b46f202a01078ee3944adf89e31ba7` |

---

# Part 1 - Is it statically clean?

## 1.0 Headline

No network client code, no exec/fork, no hardcoded hosts or IPs, no phone-home,
no license or telemetry code, no packed or encrypted blobs. The package
behaves like a PHY driver library with one exception class worth planning
around: an **unauthenticated TCP listener helper in `libsai` bound to
INADDR_ANY:8299** (present in the binary, with no caller found, so probably
dead), and a **full Lua 5.4 interpreter (os.execute/io.popen/loadlib) in
`libcrlua`** that is only reachable through the debug shell. Both are
inert unless a debug-shell path is switched on, which the harness must not
do. Details below.

## 1.1 Verdict per binary

| binary | verdict | one-line reason |
| --- | --- | --- |
| `libsai.so.1` | **notes** | dead (unreferenced) 0.0.0.0:8299 TCP shell-server helper; dlopens `libSerdes_*.so` from `$CREDO_DEVICE_PATH` (default `./`); caller-controlled file writes; optional SIOC*MII path |
| `libcredo.so` | **clean** | core SDK; only `fopen` (rb firmware / append logs), `nanosleep`, no sockets |
| `libcrlua.so` | **notes (concerning if the Lua shell is reachable)** | embeds Lua 5.4.6 with `os.execute`, `io.popen`, `package.loadlib`, env-driven init; AF_UNIX server at `/tmp/credo-server.sock` |
| `libdriver_164_u.so` | **clean** | Rambus EIP-164 driver; only `open`/`mmap` are in an exported-but-uncalled `UMDevXSProxy_*` kernel proxy |
| `crcli` | **notes** | AF_UNIX client + `system()` shell escape for interactive users; not needed by the harness, do not ship/run |
| `libowl.so` | **clean** | exports nothing; constructor only registers device descriptors |
| `libSerdes_owl800.so` | **clean** | only `fopen`/`fread` (firmware revision), `pthread_create` (shell server thread, gated); debug entry `debug_shell` is a table entry only |
| `libSerdes_owl400.so` | **clean** | differs from owl800 by 132 bytes (see 2.1) |
| debs (2) | **clean** | no maintainer scripts at all |
| `B52b0.lz4.fw.1.94.11.bin` | **cannot be statically verified** | opaque PHY microcode image; no ELF/strings; it is data for the PHY MCU, not host code |

## 1.2 Imports (V, from `nm -D --undefined-only` and call-site scans)

Checked against the list in the task. Only these are present at all:

| import | where | purpose |
| --- | --- | --- |
| `socket/bind/listen/accept/recv/epoll_*/setsockopt` | `libsai` function `run(shell_thread_priv*)` only | the 8299 listener, see 1.5 |
| `socket(AF_INET,SOCK_DGRAM)` + `ioctl` | `libsai` `ksocket_fd()` / `sonic_mdio_read_write()` | MDIO via kernel MII ioctls, see 2.3 |
| `socket/bind/listen/accept/connect/send*/recv*` (all AF_UNIX) | `libcrlua` (`create_unix_*_socket`, `run_server`), `crcli` | Credo shell, `/tmp/credo-server.sock` |
| `system`, `popen` | `libcrlua` `os_execute`, `io_popen` (Lua stdlib); `crcli` `main` | interactive shell escape / Lua stdlib |
| `dlopen/dlsym/dladdr` | `libsai` `platfrom_library_load` / `platfrom_library_procaddr`; `libcrlua` `lookforfunc` (Lua `package.loadlib`) | see 1.3 |
| `fopen` / `fopen64` | see 1.4 | |
| `mmap`, `open` | `libdriver_164_u` `UMDevXSProxy_Map` / `UMDevXSProxy_Init` | `//dev//driver_ks_c` node, see 1.6 |
| `getenv/setenv/unsetenv` | `libsai` (`platform_env_*`), `libcrlua` (Lua `os.getenv`, `setpath`, `pmain`) | |
| `chmod`, `unlink`, `mkdir`, `rename`, `remove`, `symlink`, `link`, `utime`, `mkstemp64`, `tmpfile64` | `libcrlua` (Lua `os`/`lfs`/linenoise) | `chmod` is only the readline history file |

Not imported by any binary (V): `getaddrinfo`, `gethostbyname*`, `exec*`,
`fork`/`vfork`/`clone`, `posix_spawn`, `ptrace`, `setuid`/`setgid`, `capset`,
`prctl`, `syscall`, `getrandom`, `/dev/mem` access, `chroot`, `mount`,
`kill` (except `crcli` signalling the shell server PID, see 1.5).
`libcredo` uses `rand_r` only (non-crypto jitter). AES appears only in
`libdriver_164_u` (`AES_Encrypt`, `SABuilder_AESCallback_t`) as the MACsec
SA builder needs; no RNG.

## 1.3 dlopen: what it opens (V)

`libsai` `CredoHALInterface::LoadAllDevices()` and `LoadDeviceByPHYName()`
call `getenv("CREDO_DEVICE_PATH")`, **default `"./"`** (the current working
directory), enumerate the directory with `opendir/readdir`, match the regex
`/libSerdes_.*[.]so$`, `realpath` the hit and `dlopen` it. It then `dlsym`s
`phy_api_initialize`, `phy_api_uninitialize`, `phy_api_query` ("Incorrect
device library" otherwise). SONiC sets `CREDO_DEVICE_PATH=/usr/lib`
(`supervisord.conf.j2`, V).

Consequence: any file named `libSerdes_*.so` in `$CREDO_DEVICE_PATH` (or in
the CWD if unset) is loaded and executed. The harness must set
`CREDO_DEVICE_PATH` explicitly to a read-only directory containing only the
wanted `libSerdes_owl800.so`, and run from an empty CWD.

`libSerdes_owl*.so` pull `libowl.so`, `libcredo.so`, `libcrlua.so`,
`libdriver_164_u.so` by `DT_NEEDED`, resolved through the normal search path
(no RPATH/RUNPATH anywhere, V), so `LD_LIBRARY_PATH` must point at a clean
directory.

Lua `package.loadlib` in `libcrlua` can dlopen arbitrary paths, but only from
Lua code (see 1.7).

## 1.4 File writes and paths (V from call-site scan)

| lib | function | mode | path comes from |
| --- | --- | --- | --- |
| `libsai` | `credo_sai_dbg_generate_dump` | w | caller-supplied |
| `libsai` | `switch_profile_json_t::LoadProfileFromFile`, `SaveProfileToFile` | r/w | profile key `SAI_INIT_CONFIG_FILE` |
| `libsai` | `port_profile_json_t::LoadProfileFromFile`, `tinyxml2::callfopen` | r | `SAI_HW_PORT_PROFILE_ID_CONFIG_FILE`, XML profile |
| `libsai` | `save_/load_warmboot_state_to_file`, `save_/load_switch_database_to_file` | w/r | `SAI_WARM_BOOT_WRITE_FILE` / `_READ_FILE` |
| `libcredo` | `cr_firmware_load*` | rb | `SAI_SWITCH_ATTR_FIRMWARE_PATH_NAME` |
| `libcredo` | `cr_data_capture_file`, `cr_display_info_file` | a | caller-supplied |
| `libowl` | `owl_slice_save_setup`, `common_slice_load_setup`, `owl_fw_download_from_file` | w / r / rb | caller-supplied |
| `libSerdes_owl800` | `firmware_revision` | r | firmware path |
| `libcrlua` | `lshell_tee`, `cr_slash_command_file`, `linenoiseHistorySave` (also `chmod`), Lua `io`/`os` | various | shell only; history `./.crsh_history` (crcli) |

Every write path is caller-supplied or profile-supplied; nothing writes to a
fixed absolute path except the shell socket `/tmp/credo-server.sock` and
`/tmp/lua_XXXXXX` (mkstemp in the Lua shell). No hardcoded `/etc/sonic`,
`/usr/share/sonic`, `/var/run`, `/usr/etc/credo` strings exist in the
binaries (V: string scan); those paths live only in SONiC's config files.

## 1.5 The debug port, "8299" (V unless marked)

The task statement says `crcli` connects to 8299 through `crshell`. What the
files actually show:

1. **`crshell` (SONiC, `platform/components/docker-gbsyncd-credo/crshell`)
   is a 15-line Python script**: connects TCP to `127.0.0.1:8299`, sends the
   literal bytes `debug_shell`, sleeps 1 s, then `call(["/usr/bin/crcli"])`.
   So 8299 is the trigger and `crcli` is a separate local client. `crcli`
   itself does **not** touch TCP: every socket in `crcli` is AF_UNIX
   (`create_unix_*`), default `/tmp/credo-server.sock` (string in binary).
2. **The thing that listens on 8299 is inside `libsai.so.1`**:
   `credo_shell_thread.cpp` (`shell_thread_init/run/suspend/resume/terminate/
   uninit` + `run(shell_thread_priv*)`). `run()` does `socket(AF_INET,
   SOCK_STREAM)`, `set_nonblocking`, `set_reuseaddr`,
   `sockaddr_in{AF_INET, htons(0x206b=8299), htonl(0)}`, so **`bind` to
   0.0.0.0:8299 (INADDR_ANY, all interfaces)**, `listen(4096)`, epoll loop,
   `accept`, then forwards bytes to user callbacks `on_connect/on_receive/
   on_disconnect` (`shell_callback_t`). No authentication.
3. **No caller exists in this build.** `shell_thread_init` and the other entry
   points are hidden-visibility functions with no direct call, no
   `lea`/function-pointer reference, no relocation referencing them, and they
   are not exported, `.init_array` does not contain them (I: dead code in
   1.2.12, V for the xref facts). The live path for "debug shell" is:
   `SAI_SWITCH_ATTR_SWITCH_SHELL_ENABLE` (default false) ->
   `CredoSwitch::EnableDebugShell()` -> HAL command ->
   `CredoHALDevice::EnterDebugShell()` -> `libSerdes_*::debug_shell()` ->
   `phy_driver_debug_shell` -> thread `nonblocking_shell_server` ->
   `cr_shell_spawn_server` in `libcrlua` -> AF_UNIX listener
   `/tmp/credo-server.sock` + Lua shell. That is **local-only**.
   (Who feeds `debug_shell` from the TCP port in SONiC is not visible in these
   files; I: the 8299 helper is probably wired in a different build or by the
   caller of libsai. It is not in these binaries.)
4. **Can it be disabled?** In this build: yes by construction: do not set
   `SWITCH_SHELL_ENABLE`, and nothing starts 8299 (I, no caller). Hard
   guarantee at run time: `bwrap --unshare-net` (and private `/tmp`), so even a
   0.0.0.0 bind lands in an empty netns. There is no setting that changes the
   bind address (hardcoded INADDR_ANY, V).
5. **For a later run on the switch under qemu-user**: qemu-user shares the host
   network namespace. If the listener were ever started it would be on
   0.0.0.0:8299 on the management network. Run under `unshare -n` (or a
   netns without a route) and/or add an nft drop for 8299. Treat as concerning
   only if SHELL_ENABLE or an unseen caller exists.

## 1.6 Other strings and structure (V)

* URLs/hosts: only comments/HTML in `libcrlua`'s embedded Lua (plotly,
  highlight.js, uikit on cdnjs/jsdelivr, lua-users wiki, GitHub links) in
  Penlight/luaunit/plotting helper code, plus
  `python -m http.server 4560 ... webbrowser http://127.0.0.1:4560/` in a
  Lua plot helper gated on `CREDO_DEV_ENV`. No IPs outside `127.0.0.1`. No
  hostnames in `libsai/libcredo/libowl/libSerdes/libdriver`.
* No licensing, activation, telemetry upload, update check, or call-home.
  `create_tam_telemetry`/`get_tam_telemetry_attribute` strings are SAI TAM
  API names (dlsym lookups, no networking). `ECID`/efuse read is a diagnostic
  register read.
* No base64 or encrypted payloads: entropy scan of every non-debug data
  section (4 KiB windows): max 6.26 bits/byte (`libcrlua .rodata`, Lua
  source/bytecode), no window above 7. `libcrlua` `luaopen_b64` is the
  stock base64 Lua module.
* Paths: build-host strings `/__w/1/s/...` (Azure pipeline), `/usr/include/...`
  (DWARF). Fixed runtime paths: `/tmp/credo-server.sock`, `/tmp/lua_XXXXXX`,
  `./.crsh_history`, `//dev//driver_ks_c`, default device path `./`.
* ELF: all x86-64, PIC, no RWX `LOAD`, `GNU_STACK` RW (non-exec), no `TEXTREL`,
  no `RPATH`/`RUNPATH`, no odd sections (`libsai` and both `libSerdes`
  carry `.debug_*`; `libsai` is not stripped). `.init_array` contents (V,
  resolved to symbols): `frame_dummy` plus, in `libsai`, ten
  `_GLOBAL__sub_I_*` C++ static initialisers and `lib_initialize()` ->
  `credo_library_initialize()` -> `base_rwlock_create` (a session lock);
  `libcrlua` `shell_init` (mutex init); `libowl` `hal_register_sdk`
  (registers `owl_400_dev` and `owl_800_dev` with `cr_register_device`);
  `libSerdes` `lib_initialize`. Nothing touches I/O at load time.
* `NEEDED`: `libsai`: libstdc++, libgcc_s, libc. `libcredo`: librt, libc.
  `libcrlua`: libdl, libc. `libdriver_164_u`: libc. `libowl`: librt, libc.
  `libSerdes_*`: libowl, libcredo, libcrlua, libdriver_164_u, libm, libc.
  Needed glibc: 2.34 (`libsai`, `libdriver_164_u`, `libSerdes`), libstdc++
  GLIBCXX_3.4.29 / CXXABI_1.3.9. (So not runnable inside the 2021 vendor
  chroot's glibc 2.25 even if it were arm64.)

## 1.7 libcrlua: can it run arbitrary scripts? (V, I where marked)

Yes, by design: it is a full Lua 5.4.6 (`crluaL_openlibs`, standard `os`,
`io`, `package`) with `os.execute` (`system`), `io.popen`, `package.loadlib`
(`dlopen`), `dofile/loadfile/require`. It also exports the whole Lua C API
(`lua_*`) and `cr_lua_exec`, `cr_lua_exec_full`, `crlua_run_command`.
Script sources: (a) the shell server, which executes lines read from the
AF_UNIX socket `/tmp/credo-server.sock` (`@input:%s` protocol; no
authentication, I), (b) env vars `CREDO_LUA_PATH`, `CREDO_LUA_CPATH`,
`CREDO_LUA_INIT` read at Lua state creation (`setpath`, `pmain`), (c) a
slash-command file helper (`cr_slash_command_file`, append mode). It is **not**
opened on the normal SAI path: `libSerdes` imports only
`cr_shell_spawn_server`/`cr_shell_set_slices`, called from
`phy_driver_debug_shell` (gated, 1.5). `libowl` and `libcredo` do not import
any `lua_*`/`cr_lua_*` symbol. The `HWL_RunCommand` weak imports in
`libcrlua` are unresolved unless some host provides them (I: not provided here).
Risk is therefore contingent on the shell being switched on; the harness must
also scrub the environment (`CREDO_LUA_*`, `CREDO_DEV_ENV`, `LUA_*`).

## 1.8 `.deb` maintainer scripts (V)

Both `control.tar.gz` members contain only `control` and `md5sums`: **no
preinst/postinst/prerm/postrm/triggers/conffiles**. Control: `Architecture:
amd64`, `Package: saicredo-saicredo` / `saicredo-owl`, `Version: 1.2.12`,
`Maintainer: pony.wang@credosemi.com`, `Description: Simple C++ application`,
no `Depends`. `data.tar.gz` contents are exactly the files listed in the task
(plus the `libsai.so` and `firmware/owl` symlinks); nothing is installed outside
`usr/`.

## 1.9 Part 1 summary

Clean for static purposes. The two items to carry into the harness design:
(1) `CREDO_DEVICE_PATH` default `./` -> dlopen of any `libSerdes_*.so`,
(2) latent INADDR_ANY:8299 TCP helper (dead) and the Lua shell reachable only
via `SWITCH_SHELL_ENABLE`. Neither is a reason not to run it in a netns/private
`/tmp` sandbox. Residual unknown: the FW blob is opaque and the actual
MDIO traffic cannot be verified statically; hence the fake-PHY step.

---

# Part 2 - Does it plausibly fit our hardware?

**Verdict: plausible-yes for identification and the MDIO model (strong static
evidence), unclear for run-time behaviour (firmware compatibility, slice type
0x204, devad conveyance).** Nothing seen contradicts it.

## 2.1 Owl 800 / CMS50216 support

* Device enums (libSerdes DWARF, V): `CR_DEV_OWL_400 = 0x2001`,
  `CR_DEV_OWL_800 = 0x2002`; `DEVICE_OWL400 = 0`, `DEVICE_OWL800 = 1`.
  `libowl.so`'s constructor registers both `owl_400_dev` and `owl_800_dev`
  (`hal_register_sdk` -> `cr_register_device`). `libcredo`'s
  `cri_device_get_type_name` has `owl_800` / `owl_400` names. The marketing
  name "CMS50216" appears nowhere in the binaries (V).
* **ID check (V)**, `libowl` `owl_slice_get_type()`:
  1. `owl_slice_get_chip_id()` reads reg `0xf0`; accepts `0x10` or `0x20`
     (`(x-0x10) & ~0x10 == 0`), else logs and returns `0xffff`.
     `Chip version: %04X` is the log. Our `0020` is accepted. `0x10` maps to
     slice type `0x201`. In `owl_slice_data_init`, chip `0x20` sets an extra
     flag bit (`cmp $0x20` at `0x18641`), so 0x20 gets specific handling.
  2. `canary_get_version_id()` reads regs `0xca`, `0xcb`, `0xcc` and forms
     `id = cc<<32 | cb<<16 | ca`. It then requires `id >> 20 == 0x216100a`,
     i.e. **`1.00cc == 0x2161` and `1.00cb` high nibble `0xa`**; any other
     value returns -1. Our `cc = 0x2161`, `cb = 0x00a2` passes. This is the
     same fact as the vendor's `phy_id 0x216100a0` + mask (low nibble
     `0xa2 & 0xf` is the revision).
  3. `(cb & 0xf)`: 0 -> slice type `0x202`, 1 -> `0x203`, 2 -> `0x204`,
     other -> `0x200`. **Ours is `0x204`.** Whether anything downstream
     branches specially on `0x204` is not visible (no literal compare found);
     treat as unclear (I).
* `libcredo` `cr_device_detect_type` (a generic detector used with push/pull
  access): reads reg `0xcc`; `== 0x2161` returns type `0x2001` (Owl 400); other
  chips use `0x70c2`, `0x61c6`, `0xc2`, `0x70cc`, `0x1c6==0xa2a1`. **This
  detector would report an Owl 800 as `CR_DEV_OWL_400`.** The libSerdes path
  does not rely on it: `phy_driver_allocate_device` calls
  `cr_device_create(sdk, type_from_driver)` with the type the loaded
  `libSerdes` passes (0x2001 or 0x2002). `libowl` `owl_slice_data_init` has
  `cmpl $0x2002` -> sets flag bit 1 and field `0x4b8 = 1` (800-specific).
  So the correct approach is **load `libSerdes_owl800.so`**, not rely on
  detection. (V for the compares; I for the consequences.)
* **`libSerdes_owl400.so` vs `libSerdes_owl800.so`: not identical.** Same
  size (525,912) and 132 differing bytes (V, `cmp -l`):
  - build-id (20 B) and `DT_SONAME`/`DT_NEEDED` string `owl400`->`owl800`;
  - `.rodata` string `owl400`/`owl800` (the PHY name returned by
    `get_phy_name`, matched against the `<name>` of the profile);
  - **one code immediate** in `driver_initialize()` at `.text+0x730c`:
    `movl $0x0` (owl400) vs `movl $0x1` (owl800) written into the config
    passed to `phy_driver_initialize` (= `DEVICE_OWL400` / `DEVICE_OWL800`);
  - the device-type constant `0x2001` vs `0x2002` selected in
    `phy_driver_allocate_device`/`cr_device_create` follows from that config;
  - DWARF-only differences: source path `.../owl/owl400_api.cpp` vs
    `owl800_api.cpp`, line-table file names, a few DWARF ref offsets.
  The role of `libSerdes_owl800.so`: it is the **SAI->SDK adapter** for the Owl
  800 (exports only `phy_api_initialize/query/uninitialize`), implementing
  `allocate_device`, `configure_port`, `configure_phy`, MACsec glue
  (`MACsec_*`, `IPsec_*`, SecY/CfyE calls) and wrapping `libowl`/`libcredo`.
  Arista ships `<name>owl400</name>` in its profile, so for our part the
  profile `<name>` must be **`owl800`**.
* Lane count: the 800 build takes `cr_device_get_slice(device, 0)` once per
  `allocate_device` (one slice per device). That fits our model of "one SAI
  switch per MDIO master = one slice" (16 switches, I).

## 2.2 Firmware: `1.94.11` vs `1.92.2` (V for structure, I for compatibility)

| | new `B52b0.lz4.fw.1.94.11.bin` | vendor `B52b0.lz4.1.92.2.bin` |
| --- | --- | --- |
| size | 57,664 | 57,976 |
| sha256 | `92ba4433...ba7` | `203537de...6c1` |
| layout | `0xff` fill up to `0xff0`, then image | `0xff` fill up to `0x1000`, then image |
| header words (BE) | `560ecde1 c0beffff 00006894 00006894 02902b9e 06f84f03 01000038 0000d128 01012ecc 01000000 19d10000 4f000002 d004001e` | `022e4303 a0b34914 01000038 0000d260 01012d94 01000000 54d20000 4f000002 d004001f` |
| tail | `... 01 00 00 3c 00 00 00 00 86 1d c0 fd 00 00 00` | `... 01 00 00 3c 00 00 00 00 16 d7 fc f2` |

Same family and target: same `B52b0.lz4` name root, same record skeleton
(`01000038 0000dNNN 0101xxxx 01000000 ..00 4f000002 d004001x`), same
trailer marker `01 00 00 3c 00 00 00 00` + 4-byte value (a CRC-like word; no
CRC32 relation found, so it is unverified). The new image has a 16-byte extra
preamble (`560ecde1 c0beffff` + two `0x6894` size-like words), i.e. a slightly
newer container revision (I). Neither file carries a printable version
string; the `1.94.11`/`1.92.2` come from the file names. The `lz4` in the name
is not an LZ4 frame (no `04 22 4d 18` magic): it is a raw image that the
loader streams (I). The loader is `cr_firmware_load*` in `libcredo`
(`fopen "rb"`), with version/hash read back from the PHY after loading
(`cr_firmware_version/_hash/_magic/_crc/_date`). The vendor also ships
`release_B52b0.lz4.fw.1.92.1.bin` (56,756 B) with the same skeleton.
Whether PHY RAM images are interchangeable between the 2021 and 2026 host
stacks is untested; since the PHY loads firmware into RAM at every boot, it
is reversible by reloading 1.92.2 (from the earlier note, I).

## 2.3 MDIO model

### 2.3.1 Two access modes, selected by `SAI_SWITCH_ATTR_SWITCH_HARDWARE_INFO` (V)

`CredoSwitch::InitializeDevice(bool)` (libsai `0x1f2700`):

* reads `SAI_SWITCH_ATTR_TYPE` (must be `SAI_SWITCH_TYPE_PHY` = 1, else
  "Unsupported switch type");
* reads `PLATFROM_CONTEXT` (u64, note the header's spelling), `HARDWARE_ACCESS_BUS`,
  `SWITCH_HARDWARE_INFO`;
* **if hardware-info is non-empty** -> `sonic_set_phy_to_mdio(ctx, hwinfo)` and
  the read/write functions are libsai's own `sonic_mdio_read/write`
  (kernel MII ioctls, below); `REGISTER_READ/WRITE` are ignored;
* **else** the user-supplied `SAI_SWITCH_ATTR_REGISTER_READ` /
  `REGISTER_WRITE` function pointers are used.
  It fills `DeviceConfig_t`, then dispatches HAL command `0x4c` -> `phy_driver_allocate_device`.

`DeviceConfig_t` (DWARF, 0x130 bytes): `platform_context u64 @0`,
`phy_mode @8`, `phy_id @0xc`, `phy_addr @0x10`, `hw_info[256] @0x14`,
`register_table{register_read,register_write} @0x118`, `warm_recover @0x128`.
`phy_id`/`phy_addr` come from the profile (`SetPHYName/Mode/Addr`).

### 2.3.2 Callback signatures (V, saiswitch.h + DWARF)

Set on `sai_create_switch` via
`SAI_SWITCH_ATTR_REGISTER_READ` (id 0xa2, `sai_pointer_t`) and
`SAI_SWITCH_ATTR_REGISTER_WRITE` (id 0xa3):

```c
typedef sai_status_t (*sai_switch_register_read_fn)(
    uint64_t platform_context,   // SAI_SWITCH_ATTR_PLATFROM_CONTEXT
    uint32_t device_addr,        // DeviceConfig.phy_addr
    uint32_t start_reg_addr,
    uint32_t number_of_registers,
    uint32_t *reg_val);          // out
typedef sai_status_t (*sai_switch_register_write_fn)(
    uint64_t platform_context, uint32_t device_addr,
    uint32_t start_reg_addr, uint32_t number_of_registers,
    const uint32_t *reg_val);
```

The libSerdes glue `csdk_register_read/write` (V disassembly) calls them as
`fn(ctx->platform_context, ctx->phy_addr, reg, 1, &val)`:
**one register per call** (n = 1) from this path. `reg` is the SDK register
number (16-bit space, e.g. `0xcc`, `0xcb`, `0xca`, `0xf0`, TCM/hive regs).
There is **no clause-22/45 flag, no devad and no slice argument** in the
callback; **the callback owns the translation to MDIO**. The vendor's
observation `1.00cc = 0x2161` implies devad 1 (PMA/PMD) reg `0x00cc`, so the
callback should issue clause-45 reads at devad 1 (I; consistent with
`cr_device_detect_type` reading reg `0xcc`).

The typedefs named in the task, `sai_switch_mdio_read_fn/write_fn` and
`sai_switch_mdio_cl22_read_fn/write_fn` (`switch_id, device_addr,
start_reg_addr, number_of_registers, reg_val`), are **slots in
`sai_switch_api_t`** (`switch_mdio_read`, `switch_mdio_write`,
`switch_mdio_cl22_read`, `switch_mdio_cl22_write`), i.e. host-facing entry
points that libsai implements, **not** callbacks libsai calls. `switch_mdio_read/write`
(`credo_sai_switch_mdio_read/write`, V) look up the switch's
`GetRegisterReadFunction()`/`Write` and forward to the same
REGISTER_READ/WRITE pointers. No implementation symbol exists for the cl22
slots (I: NULL/unsupported). No `SAI_SWITCH_ATTR_*` carries MDIO callbacks
other than REGISTER_READ/WRITE.

### 2.3.3 libsai's own MDIO path (hwinfo set), and the "exceeds maximum (31)" message (V)

`sonic_mdio_read_write(platform_context, mdio_addr, base_reg_addr,
number_of_registers, read_data, write_data)` (`credo_sai_sonic_mdio.cpp`):

1. `ksocket_fd()` = `socket(AF_INET, SOCK_DGRAM, 0)`;
2. interface name = `SWITCH_TO_MDIO_MAP[platform_context]`, filled by
   `sonic_set_phy_to_mdio` from hwinfo `"<netdev>/<phy_id>"` (header comment: the
   MDIO SysFS driver case; SONiC uses e.g. `mdio0_0_0/0`);
3. `ioctl(SIOCGMIIPHY=0x8947)` on that netdev returns `phy_id` of the bus's
   first PHY; decoded as `base_prtad = (id>>5)&0x1f`, `devad = id&0x1f`,
   `c45 = id & 0x8000`;
4. `slice_prtad = (base_prtad == 0) ? mdio_addr : base_prtad + mdio_addr`;
   if `> 31`: `"Calculated slice PHY address %u exceeds maximum (31) for
   mdio_addr=%u, base_prtad=%u"`;
5. `slice_phy_id = ((slice_prtad<<5)&0x3e0) | (devad&0x1f) | c45`;
6. per register: `SIOCGMIIREG (0x8948)` / `SIOCSMIIREG (0x8949)` with
   `struct mii_ioctl_data {phy_id, reg_num = base_reg_addr + i, val_in, val_out}`.

So **it is clause-45 only** (devad/c45 inherited from the interface's
configured `phy_id`), and the address model is "bus base PHY address + slice
index", with `mdio_addr` = `DeviceConfig.phy_addr` (the profile `phy_addr`,
0 in Arista's XML). For us, bus N (`mdio-N`) has one PHY at address N+1. A
custom callback sidesteps this entirely, which is the plan.

### 2.3.4 Attributes needed to create a PHY switch (V: header + InitializeDevice/Configure/DownloadFirmware)

| attribute | id | notes |
| --- | --- | --- |
| `SAI_SWITCH_ATTR_INIT_SWITCH` | 0x74 | mandatory. **`false` makes `Configure()` return immediately: no HAL device, no profile load, nothing allocated** (V). Not a usable "attach". |
| `SAI_SWITCH_ATTR_TYPE` | 0xaf | `SAI_SWITCH_TYPE_PHY` |
| `SAI_SWITCH_ATTR_HARDWARE_ACCESS_BUS` | 0xa0 | `SAI_SWITCH_HARDWARE_ACCESS_BUS_MDIO` (read; does not change the callback/ioctl selection) |
| `SAI_SWITCH_ATTR_PLATFROM_CONTEXT` | 0xa1 | u64 cookie handed back to callbacks (use bus index 1..16) |
| `SAI_SWITCH_ATTR_REGISTER_READ` / `_WRITE` | 0xa2 / 0xa3 | mandatory when hwinfo empty |
| `SAI_SWITCH_ATTR_SWITCH_HARDWARE_INFO` | 0x72 | **leave empty** to avoid the ioctl path |
| `SAI_SWITCH_ATTR_SWITCH_PROFILE_ID` | 0x71 | u32 handle passed to `profile_get_value` (keys below) |
| `SAI_SWITCH_ATTR_FIRMWARE_PATH_NAME` | - | s8 list; empty -> "Firmware path is empty, skipping firmware load" (V string) |
| `SAI_SWITCH_ATTR_FIRMWARE_LOAD_METHOD` | 0xa5 | `NONE/INTERNAL/EEPROM`; read by `DownloadFirmware()` (I: NONE skips) |
| `SAI_SWITCH_ATTR_FIRMWARE_LOAD_TYPE` | - | `SKIP/FORCE/AUTO` (default AUTO: "if version differs, load"); use for no-reload attach (honoured by libSerdes, not verified here) |
| `SAI_SWITCH_ATTR_SLAVE_MDIO_ADDR_LIST` + `FIRMWARE_DOWNLOAD_BROADCAST/EXECUTE/...` | - | broadcast firmware download only, **not needed** (16 independent masters) |
| `SAI_SWITCH_ATTR_SWITCH_SHELL_ENABLE` | - | **keep false** (see 1.5) |

`sai_service_method_table_t.profile_get_value(profile_id, key)` must answer
(V, strings): `SAI_INIT_CONFIG_FILE` (switch profile, XML or JSON),
`SAI_HW_PORT_PROFILE_ID_CONFIG_FILE` (per-port profile; else "'...' is null
for profile id"), `SAI_BOOT_TYPE` (`cold` / `warm` / `fast`),
`SAI_WARM_BOOT_READ_FILE`, `SAI_WARM_BOOT_WRITE_FILE`.

### 2.3.5 Attach to an already-initialised PHY without reloading firmware

Two candidates (V for code existence, I for behaviour):

* **Boot type `fast`** (`SAI_BOOT_TYPE=fast` -> `CredoSwitch::FastBoot()`):
  `ReadProfile` -> `InitializeDevice` -> `GetFirmwareStatus`; log
  `"firmware is not running, skip fast boot"` if the MCU is not up; otherwise
  skips `DownloadFirmware`. This is the designed "firmware already running"
  path.
* **Cold boot with `FIRMWARE_LOAD_TYPE=SKIP`** or empty firmware path or
  `LOAD_METHOD=NONE`: `InitializeDevice` then `SetOptions/SetSerdesParams/
  ConfigurateLanePolarity/...` as in `ColdBoot()`.
* `warm` is for restoring saved SAI state from `SAI_WARM_BOOT_READ_FILE`
  (file-based; also needs `warm_recover`, which maps to the new
  `WarmBoot_2Stage_*` driver calls); not an attach.
* `INIT_SWITCH=false` is **not** an attach in this build (above), despite the
  header comment "Connect to SDK".

## 2.4 DWARF-recovered interfaces worth keeping

* `DeviceConfig_t`, `DeviceCapabilities_t` (`max_lanes, max_ports,
  num_of_host_lanes, num_of_line_lanes, acl_entry_{min,max}_priority,
  features[2], synce_clock_count`), `phy_register_method_table_t`,
  `FECMode_t`, `Side_t` (`HOST_SIDE/SYSTEM_SIDE=0, LINE_SIDE=1`) are in the
  DWARF of `libsai.so.1`. Dump with
  `llvm-dwarfdump --debug-info libsai.so.1` (about 130 MB text; grep per name).
* HAL command set (strings, `libsai`): `load_firmware`, `get_firmware_info`,
  `get_firmware_status`, `set_device_option`, `set_port_option`,
  `set_lane_option`, `get/set_lane_polarity`, `get/set_lane_tx/rx_polarity`,
  `get/set_tx_taps`, `get/set_tx_taps_scale`, `get/set_lane_tx/rx_precoder`,
  `get/set_rx_dfe`, `get/set_rx_ffe`, `set_fast_link_recovery`,
  `configure_loopback`, `configure_port_prbs`, `debug_shell`,
  `serdes_parameter{i,f,listi,listf}`, `configure_port[_ex]`,
  `configure_phy`, `allocate_device`.

## 2.5 MACsec (V)

SAI objects (saimacsec.h, serialised in `libsai`): MACSEC, MACSEC_PORT,
MACSEC_FLOW, MACSEC_SC, MACSEC_SA, with ACL tables for flow match and
IPsec objects (`saiipsec.h`) in the same library. Cipher suite enum:
`GCM_AES_128`, `GCM_AES_256`, `GCM_AES_XPN_128`, `GCM_AES_XPN_256`.
`libSerdes` capability flags: `SupportGCM_AES128`, `SupportGCM_AES256`,
`SupportXPN64Bit`/`f64BitsXPN`, `SupportIngressSCI`; SA builder fields
`SSCI`, `Salt`, `XPN64Enable`, `ConfidentialityOffset`, `ICVByteCount`;
which of these are actually advertised comes from
`SAI_MACSEC_ATTR_SUPPORTED_CIPHER_SUITE_LIST` at run time (not static).

HAL commands -> libSerdes functions -> driver calls (call graph from
`libSerdes_owl800.so` disassembly, V):

| SAI/HAL | libSerdes | driver calls |
| --- | --- | --- |
| SA create | `MACsec_SA_Create`, `macsec_build_sa` | `SABuilder_InitParams/BuildSA/GetSize`, `SecY_SAIndex_Get`, `SecY_SA_Add`, `SecY_SA_Chain`, `SecY_SA_Active_E_Get`, `SecY_SA_Switch`, `SecY_SA_Remove`; `CfyE_vPort_Read/Update` |
| SA rekey (`macsec_sa_rekeying`) | `MACsec_SA_Rekeying` | `SecY_SA_Chain`, `SecY_SA_Switch`, `SecY_SA_Remove` (hitless switch wired) |
| SA remove | `MACsec_SA_Remove` | `SecY_SAHandle_IsSame`, `SecY_SA_Active_E_Get`, `SecY_SA_Chained_Get`, `SecY_SA_Remove`, `SecY_SA_Switch` |
| SA update, PN/XPN | `MACsec_SA_Update`, `_Update_NextPN`, `_Get_NextPN`, `macsec_sa_get_xpn`, `get/update_minimum_xpn` | `SecY_SA_Update`, `SecY_SA_NextPN_Update/Get` |
| replay window, SCI, pre-SecTag auth | `MACsec_SA_Enable_WindowSize`, `_Update_WindowSize`, `_Enable_SCI`, `_Set_PreSecTagAuthStart` | `SecY_SA_WindowSize_Update`, `SecY_SA_Params_Read` + `SecY_SA_Update` |
| SA stats | `MACsec_SA_Get_E_Stat`, `_Get_I_Stat`, `_Clear_Stat` | `SecY_SA_Statistics_E/I_Get`, `SecY_SA_Statistics_Clear` |
| SecY / interface stats | `MACsec_vPort_Get_SecY_Stat`, `_Get_Ifc_Stat`, `_Clear_*` | `SecY_SecY_Statistics_E/I_Get`, `SecY_Ifc_Statistics_E/I_Get`, `SecY_*_Statistics_Clear` |
| rules (flow match) | `MACsec_Rule_Create/Update/Remove/Enable/Disable/Get_Stat/Clear_Stat` | `CfyE_Rule_Add_Index/Update/Remove/Enable/Disable`, `CfyE_Statistics_TCAM_Get/Clear` |
| vPort/channel | `MACsec_vPort_Create/Remove/Update*`, `MACsec_Channel_*` (bypass, EOP/SOP insertion, stats) | `CfyE_vPort_Add/Remove`, `CfyE_Channel_Bypass_Get/Set`, `SecY_Channel_Bypass_*`, `CfyE_Device_Insert{SOP,EOP}`, `CfyE_Statistics_Channel_*`, `SecY_Channel_Config_Get/Update` |
| clear-on-read | `MACsec_Device_Enable_Clear_On_Read` | `*_Device_Config_Get/Update` |
| warm boot | `MACsec_WarmBoot_*` | `WarmBoot_2Stage_DataPath_Add/Remove/Restore/Shutdown`, `WarmBoot_Size_Get` |

So SA switch/rekey, per-SA / SecY / interface / TCAM / channel stats, XPN64, replay
window and 128/256 GCM are all wired through; the SecY/CfyE/EIP-164 layer
(`libdriver_164_u`) is the 2021 stack's own interface, extended (below), so the
earlier plan "read the DWARF'd glue and port it onto the arm64 driver" stays
valid.

`libdriver_164_u.so` exports: 2021 arm64 = 389 symbols; 1.2.12 x86-64 = 492
symbols. **Strict superset**: the only symbols present in the old lib and
absent in the new are linker markers (`__bss_*`, `_edata`, `_end`, `_init`,
`_fini`). 112 new exports: warm boot (`CfyELib_WarmBoot_*`,
`Adapter_Init_WarmBoot_*`, `Driver164_ReInit`), MTT (`CfyE_MTT_*`,
`EIP163_MTT_*`, MTT stats), TCAM/channel stats-clear, IPsec stat gets
(`EIP164_SecY_Stat_IPsec_*`), `SecY_Channel_Config_Update`,
`SecY_Device_Limits_Get`, `SecY_Ifc_Statistics_Clear`, `CfyE_vPort_Add_Index`,
`output_log_message`, `Driver164_*_GetProperties`, `AES_Encrypt`. ABI and
behavioural notes: struct layouts are not recoverable for the arm64 lib (no
DWARF) so signature compatibility of the common functions is **unverified**;
the host-side libs also differ a lot (the 2021 `libcredo` exports an
unprefixed API, `allocate_device/create_sdk/firmware_load/...`, 518 symbols,
while 1.2.12 `libcredo` exports 494 `cr_*` symbols: no drop-in mixing of old
and new host libs, I). `libowl` went from 9 exports (2021) to none (registers
via constructor).

## 2.6 SONiC's public Owl configuration format (Arista 7280CR3MK-32P4)

(`sonic-net/sonic-buildimage`, `device/arista/x86_64-arista_7280cr3mk_32p4/Arista-7280CR3-C40/`,
fetched with `gh api`; Arista uses **Owl 400**.)

* `psai.profile`: `SAI_INIT_CONFIG_FILE=/usr/share/sonic/hwsku/gearbox_100G_PAM4.xml`.
* `context_config.json`: context `"phys"` lists `switches[i].hwinfo =
  "mdio<i>_0_0/0"` (netdev `/` phy_id: the ioctl path), `dbAsic GB_ASIC_DB`.
* `gearbox_config.json` `phys[]`: `phy_id, name "phyN", address "N",
  lib_name "", firmware_path "/usr/etc/credo/firmware/owl", config_file
  ".../phyN_config.json", sai_init_config_file "", phy_access "mdio", bus_id,
  context_id, macsec_ipg 352, hwinfo`. `interfaces[]`: `name, index,
  phy_id, system_lanes [4,5], line_lanes [20..23],
  {system,line}_tx_fir_{pre2,pre1,main,post1,post2}` per-lane arrays.
* `phyN_config.json`: `lanes[] {index, local_lane_id, system_side,
  tx_polarity, rx_polarity, line_tx_lanemap, line_rx_lanemap,
  line_to_system_lanemap, mdio_addr}` and `ports[] {index, system_speed,
  system_fec, line_speed, line_fec, *_auto_neg, *_loopback, *_training,
  line_media_type, ...}` (this is the gbsyncd's own gearbox schema, not read by
  libsai).
* `gearbox_100G_PAM4.xml` (**the one libsai reads**; `credo_profile_xml.cpp`):
  `<root><name>owl400</name><phy_addr>0</phy_addr><mode>gearbox</mode>
  <topology>2</topology><lane id=N system-side="true|false"/>...
  <PAM4><lane id= tx-taps="pre2,pre1,main,post1,post2"/></PAM4>
  <NRZ>...</NRZ></root>`. Further keys the parser understands (strings in
  libsai): `tx-polarity`/`rx-polarity` on `<lane>`, `tx-taps-scale`,
  `serdes_parameters`/`serdes_parameter` (`type` int/float/int_list/float_list),
  `option`/`value`, `port-connector` with `lane-number`, `speed`,
  `system-side`/`line-side`, `host_an/host_lt/line_an/line_lt/
  host_optical/line_optical`, fec `firecode/rs528/rs544`.
  JSON profile variant (`credo_profile_json.cpp`): `version, name, phys[]
  {hw_info, mode, phy_id, phy_addr, phy_access, lane_config[{lane,
  tx_polarity, rx_polarity, tx_taps, tx_taps_scale, rx_dfe, rx_ffe,
  options}]}, ports[{speed, host_lanes, line_lanes, host_fec, line_fec}]`.
  Lane maps are therefore not a libsai-profile concept; the lane lists of
  port-connectors are.

Mapping to our vendor files (I):

| vendor (`swap_setting.cmd`, `preemphasis_*.cmd`, `dynamic_config.txt`) | libsai equivalent |
| --- | --- |
| `set_lane_config slice_id=S lane_mask=0xff0ff rx_pol_mask=.. tx_pol_mask=..` (bitmask per slice; lanes 0-7 and 12-19) | per-lane `tx-polarity`/`rx-polarity` in `<lane>` / `lane_config`; 16 profiles/XML (one per slice/bus) |
| `set_tx_taps slice_id=S lane=L taps=0,-4,18,-2,0` | `tx-taps` (same 5-tap order as SONiC `pre2,pre1,main,post1,post2`: Arista `1,-5,14,0,0`) in `<PAM4>`/`<NRZ>` |
| `set_tx_msb/set_rx_msb` | no direct XML field; candidate `serdes_parameter`/lane `option` (needs checking at run time) |
| `firmware_configure_port ... mode=MacsecBypass connect=Retimer line_start=12 host_start=0 ... speed=50000 RS544` | profile `<mode>` (`retimer`/`gearbox`/`bitmux` strings exist) + port-connector lanes + fec; SAI MACsec objects replace "MacsecBypass" |
| `allocate_device device_id=CREDO_OWL_800` | `<name>owl800</name>` + loading `libSerdes_owl800.so` |
| `slice_init ... SLICE_INIT_NO_FIRMWARE` vs `SLICE_FULL_INIT` | boot type `fast` / `FIRMWARE_LOAD_TYPE=SKIP` vs cold with firmware |
| `mdio_reg_wr slice_id=all addr=0xb73a val=0x40` | no SAI equivalent; do it from the harness's callback path (post-init), or leave out until proven needed |
| `mdc_freq` sysfs writes | not SAI; stays in the platform glue |

---

# Part 3 - What the x86 harness must provide, and risks

## 3.1 Step-0 harness (sandboxed, fake PHY) - required pieces

Process / sandbox:

* `bwrap --unshare-all` (net, pid, ipc, uts) `--die-with-parent`, read-only
  bind of exactly the lib dirs and a minimal glibc >= 2.34 + libstdc++
  (GLIBCXX_3.4.29) + libgcc_s; **no `$HOME`, no `/dev`/`/sys`/`/proc` beyond
  what the loader needs**, private tmpfs `/tmp`, a scratch tmpfs as CWD
  (empty), environment cleared. Set only:
  `LD_LIBRARY_PATH=<libdir>`, `CREDO_DEVICE_PATH=<dir with only libSerdes_owl800.so>`.
  Do not set `CREDO_LUA_*`, `CREDO_DEV_ENV`, `LUA_*`.
* Hard timeout + `RLIMIT` (the library has worker threads and a register-poll
  loop; bounded run avoids hangs).

SAI entry:

1. Link/dlopen `libsai.so.1` (exports 3208 symbols, includes
   `sai_api_initialize/query/uninitialize`, `sai_log_set`,
   `sai_dbg_generate_dump`, ...).
2. `sai_api_initialize(0, &services)` with a **`sai_service_method_table_t`**:
   `profile_get_value(profile_id, key)` and `profile_get_next_value` returning
   at least `SAI_INIT_CONFIG_FILE`, `SAI_BOOT_TYPE` (`cold` or `fast`),
   `SAI_WARM_BOOT_READ_FILE/WRITE_FILE` (unset or tmpfs paths),
   `SAI_HW_PORT_PROFILE_ID_CONFIG_FILE` if a port profile is used.
3. `sai_api_query(SAI_API_SWITCH, ...)` (and macsec/port APIs later).
4. `create_switch` attributes: `INIT_SWITCH=true`, `TYPE=PHY`,
   `HARDWARE_ACCESS_BUS=MDIO`, `PLATFROM_CONTEXT=<bus index>`,
   `REGISTER_READ`, `REGISTER_WRITE`, `SWITCH_PROFILE_ID`,
   `FIRMWARE_PATH_NAME` (empty for step 0), `FIRMWARE_LOAD_METHOD=NONE` or
   `LOAD_TYPE=SKIP`; **leave `SWITCH_HARDWARE_INFO` unset** (else the ioctl
   path runs and opens an AF_INET socket) and leave `SWITCH_SHELL_ENABLE`
   false.

Callbacks (signatures in 2.3.2). The fake PHY sits behind
`(platform_context, device_addr, reg)`:

* map `platform_context` -> bus N, return values from a register table; log
  every access (reads and writes, in order, with timestamps) to a pipe/tmpfs
  file that the harness drains;
* seed registers: `0xcc = 0x2161`, `0xcb = 0x00a2`, `0xca = anything`
  (low 16 bits of the version id), `0xf0 = 0x0020`; return `SAI_STATUS_SUCCESS`
  (0) and zero for unknown registers;
* expect `number_of_registers == 1` from the glue path;
* decide and record the clause-45 devad the real callback would use (assumed
  devad 1, I);
* expect long poll loops (firmware magic, top-PLL cal, MCU status): a pure
  logging fake will hit timeouts; that is fine for step 0 (goal is the access
  sequence); bound the run by wall clock.

Config files (read-only, in the sandbox):

* switch profile XML (or JSON) with `<name>owl800</name>`, `<phy_addr>0</phy_addr>`,
  `<mode>retimer</mode>` or the `gearbox` shape from Arista, `<topology>`, `<lane>`
  entries, polarity and taps. Start from our vendor values:
  lanes `0xff0ff` = 0-7 and 12-19; taps 5-tuples; polarities per slice from
  `swap_setting.cmd`; and port-connector lane lists.
* `libSerdes_owl800.so` alone in `CREDO_DEVICE_PATH`.

Expected step-0 outcome: library load (all dlopens succeed), device
registration, `owl_slice_get_type()` reads (`0xf0`, `0xca`, `0xcb`, `0xcc`)
and then register traffic from `cr_slice_init`/firmware status, ending in a
logged failure or a timeout. The access list is the deliverable.

## 3.2 For the real run on the switch (later)

* qemu-user x86_64 + glibc 2.34 userland in an unprivileged chroot/container;
  network namespace without routes (`unshare -n`) so 8299/the AF_UNIX shell
  cannot be reached; no `/dev/mem`, no `/dev/mdio*`.
* The MDIO bridge is a unix-socket or fd-passing helper that the arm64 side
  runs (the existing libMdio sysfs path, `PHY_mdio_read/write`, with PHY address
  N+1 on bus N) and the x86 callbacks forward over it.
* Firmware: either keep the already-loaded 1.92.2 (boot type `fast` or
  `LOAD_TYPE=SKIP`) or load 1.94.11 as a separate, reversible experiment.

## 3.3 Risks and things that should stop us from running it

Stop-ship (any one means: do not run until resolved):

1. **If any path can start the shell** (`SWITCH_SHELL_ENABLE=true`, or an
   unseen caller of `shell_thread_init`): Lua with `os.execute`/`io.popen`
   behind an unauthenticated AF_UNIX socket, plus a 0.0.0.0:8299 TCP helper.
   Mitigate with netns + private `/tmp`; never set the attribute.
2. **Not setting `CREDO_DEVICE_PATH`** (default `./`): dlopens whatever
   `libSerdes_*.so` is in CWD.
3. **Any run where `SWITCH_HARDWARE_INFO` is non-empty**: libsai opens an
   AF_INET socket and issues MII ioctls against the named netdev; on the
   switch that is real MDIO traffic with an address model that does not match
   ours.
4. Running with real access to the switch before the fake-PHY access log has
   been reviewed (writes to registers `0xb73a`-class or MCU/firmware regs).

Technical risks (not stop-ship):

* devad not conveyed by the callback (assumed 1), address mapping (bus N, PHY
  N+1 vs `device_addr`), 16 separate switches (one SAI switch per slice).
* Slice type `0x204` (rev nibble 2) behaviour unknown; the generic
  `cr_device_detect_type` maps `0x2161` to Owl 400, so the harness must pick
  `owl800` by profile/library, not by detection.
* Firmware 1.94.11 vs 1.92.2: same family/skeleton, but untested; header has a
  new preamble. Prefer `SKIP`/`fast` first.
* No clause-22 path, no hitless "connect-only" mode via `INIT_SWITCH=false`.
* Timing/polling and worker threads (`libdriver` `WorkerThread_Start`,
  `libsai` pthreads) under qemu-user performance.
* `libcrlua` pulls 6 MB of Lua and `libsai` 16 MB; everything is glibc >= 2.34.
* Vendor delivered DWARF only for `libsai` and `libSerdes_*`; `libowl`/`libcredo`/
  `libdriver` internals were read from disassembly and exported names only.
* MACsec scope: SAI API is complete; whether our hardware's EIP-164 (2021
  arm64 `libdriver_164_u`, no warm-boot symbols) matches what the 2026 glue
  expects is a separate port question (struct layouts differ unknowably from
  here).

## 3.4 Open questions

1. What invokes the 8299 listener in SONiC (not in these binaries).
2. Slice type `0x204` meaning (rev-2 silicon); any dependent firmware
   command-set differences.
3. Whether `FIRMWARE_LOAD_TYPE=SKIP`/`fast` leaves the 2021-loaded PHY MCU in a
   state 1.2.12 accepts (firmware status/version handshake).
4. Which devad the callback should use for the TCM/hive registers (assumed 1
   for everything).
