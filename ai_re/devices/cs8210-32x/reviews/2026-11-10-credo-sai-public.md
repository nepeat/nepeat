# 2026-11-10 — public Credo SAI (SONiC gbsyncd-credo) vs the vendor c-phy stack

Desk research only; nothing ran on the switch.

## Source

- `sonic-net/sonic-buildimage`, `platform/components/docker-gbsyncd-credo.mk`:
  `LIBSAI_VERSION = 1.2.12` (commit "Update Credo SAI (libsaicredo) to v1.2.12
  (#28841)", 2026-08-21). History goes back to v0.9.6 (2024-05); actively
  maintained.
- Packages are public at
  `https://packages.trafficmanager.net/public/credosai/<name>_1.2.12_amd64.deb`
  (`BUILD_PUBLIC_URL` in `rules/config`): `libsaicredo`, `libsaicredo-owl`,
  `libsaicredo-blackhawk`, `libsaicredo-crt88322`. **amd64 only** (arm64 URL
  returns 404).
- Downloaded (not redistributed; local copy in
  `analysis/research/credo-sai-1.2.12/`, git-ignored):
  - `libsaicredo_1.2.12_amd64.deb` sha256 `ca66a1ef…b5ca`
  - `libsaicredo-owl_1.2.12_amd64.deb` sha256 `91351039…765d`
- Users in SONiC: Arista 7280CR3MK-32P4, 7800R3-48CQM2 line card (Owl,
  `phy_access: mdio`, `macsec_ipg: 352`), 7280R4 (other Credo part).

## Contents (V = read from the files)

| file | notes |
| --- | --- |
| `usr/lib/libsai.so.1` (16 MB) | SAI 1.18.0 PHY SAI, **with debug_info**. Implements the SAI MACsec API (`saimacsec.h`: port/flow/SC/SA objects + stats). MDIO via SAI switch `REGISTER_READ/WRITE` callbacks or its own fd (`credo_sai_sonic_mdio.cpp`). |
| `usr/lib/libdriver_164_u.so` | Inside Secure/Rambus EIP-164/163 MACsec driver, not stripped, 492 exports. |
| `usr/lib/libcredo.so`, `libcrlua.so`, `usr/bin/crcli` | Credo core + Lua-based CLI (`crshell` connects to a debug port 8299 in the container). |
| owl pkg: `libowl.so`, `libSerdes_owl400.so`, `libSerdes_owl800.so` | Owl family support; `libowl.so` exports only 2 symbols. |
| owl pkg: `usr/etc/credo/firmware/B52b0.lz4.fw.1.94.11.bin` (57,664 B) | **Newer Owl firmware**: ours is `B52b0.lz4.1.92.2.bin` (01.92.02). |

## What it changes for us

1. **Rekeying is in our own arm64 vendor library already.** The 2021 arm64
   `c-phy/libdriver_164_u.so` exports `SecY_SA_Add`, `SecY_SA_Update`,
   `SecY_SA_Remove`, **`SecY_SA_Switch`** (hitless SA switchover),
   `SecY_SA_NextPN_*`, `SecY_SA_PnThrSummary_CheckAndClear`,
   `SecY_SA_Statistics_E/I_Get`, `SecY_SecY_Statistics_*`, `SecY_Diag_*`. **V**.
   Only `phy_shell`'s command table lacks them. Correction to the
   earlier chat answer ("no rekey/remove/per-SA counters"): the CLI has none,
   the arm64 library does.
2. **A complete reference for MKA-driven MACsec on Owl.** SONiC's flow is
   `wpa_supplicant` (MKA) → `macsecmgrd` → SAI MACsec API → `libsaicredo`
   → EIP-164 on the Credo PHY. The x86 `libsai.so.1` carries DWARF, so the
   exact sequence from SAI objects to `SecY_*`/`CfyE_*` calls can be read
   out and reimplemented as arm64 glue against our libraries (**I**: plan,
   not tried).
3. **Newer PHY firmware 1.94.11.** The firmware is loaded into PHY RAM at
   every boot, so trying it is reversible by reloading 1.92.2. Unknown
   whether our 2021 `libowl.so`/`phy_server` handles it. Untested.
4. Running the x86 `libsai` itself on the arm64 switch would need qemu-user
   plus an MDIO bridge to our arm64 `libMdio`. Possible but heavy; the RE route
   (item 2) looks better.

## Not found

No source code, datasheet or register manual for CMS50216/Owl/EIP-164 on
GitHub or Credo's site (Credo docs are customer-gated). Product briefs only.

## arm64 hunt (2026-11-10)

**Result: no aarch64 build of Credo's SAI or Owl SDK found anywhere public.
Nothing to qemu-test, so no load/export/glibc test was possible.** All
requests used the default curl/gh client, no personal data. Nothing touched the
switch.

### Checked (V = observed response)

| source | result |
| --- | --- |
| `packages.trafficmanager.net/public/credosai/{libsaicredo,libsaicredo-owl}_1.2.12_{arm64,aarch64,armhf,arm64v8}.deb` and `-<arch>` spellings (16 URLs, HEAD) | all 404 (V) |
| `https://packages.trafficmanager.net/public?restype=container&comp=list&prefix=credosai/` | **200, public listing works**, 46 blobs, no NextMarker (V). Every blob is `_amd64.deb`; zero non-amd64 (V). |
| same listing for `sonicstorage.blob.core.windows.net/public?...comp=list` | 403 AuthorizationFailure (network security perimeter) (V); direct blob GETs there also 403 |
| Listing contents | versions 0.5.2 0.7.2 0.7.5 0.8.2 0.9.0 0.9.3 0.9.6 0.9.9 1.2.4 1.2.6 1.2.10 1.2.12. Packages: libsaicredo(12), -owl(12), -blackhawk(6), -crt88322(4), -screaming(3), -baldeagle, -blueheron, -crt55321, -kingfisher, -oriole, -osprey, -dummy, -example, -unspecified (1 each). All amd64 (V). Only owl is relevant to us. |
| git history of `docker-gbsyncd-credo.mk` (21 commits, 2021-08 to 2026-08) | every version pins `_amd64.deb`; older ones used `sonicstorage.../packages/credosai/` with SAS tokens, still amd64 (V). Current file has no `arm64`/`ARCH` conditional (V). |
| sonic-buildimage platform rules | `credo` is included only from `platform/broadcom/rules.mk` under `INCLUDE_GBSYNCD`; `marvell-prestera`, `marvell-teralynx`, `nvidia-bluefield` rules.mk contain no credo/gbsyncd reference (V, raw files). No arm64 platform builds gbsyncd-credo (V for those 3; others I (not each checked)). |
| GitHub code search `credosai`, `libsaicredo`, `gbsyncd-credo`, `B52b0.lz4`, `credo_sai`, `libowl.so` | only sonic-buildimage and its forks (edge-core, TIP ols-nos, canonical archive, Marvell embedded, kalyanpullela/alpine-otel, etc.) holding the same amd64 `.mk`; `B52b0.lz4` 0 hits; `libowl.so` hits unrelated (V). Searches for sairedis/sonic-mgmt/marvell-arm64 hit GitHub rate limit, not completed (not checked). |
| Marvell fork `marvell-sonic-buildimage` | `docker-gbsyncd-credo.mk` at v1.2.10, amd64 only (V) |
| Docker Hub `library|sonicdev-microsoft/docker-gbsyncd-credo` | 404 (V). `sonicdev-microsoft.azurecr.io` and `ghcr.io` need auth, 401 (V); manifests not inspectable |
| `sonic-build.azurewebsites.net/api/sonic/artifacts` | `platform=broadcom` serves `docker-gbsyncd-credo.gz` (amd64 platform, not downloaded). `broadcom-arm64`, `marvell-arm64`, `marvell-prestera-arm64`, `marvell-teralynx` are "not defined" (V) |
| `sonic.software` | reachable, not explored further (I: no arm64 credo since no arm64 platform builds it) |
| Not checked | Celestica/DENT/ONL/Arista GitHub orgs beyond the below, PyPI/npm, Credo GitHub org (none found by code search) |

### Side finding: Arista `phy-credo_1.0_amd64.deb`
`rules/phy-credo.mk` pulls `aristanetworks/sonic-firmware/phy/phy-credo_1.0_amd64.deb`
(pinned commit 24716c4e). Downloaded to
`analysis/research/arista-phy-credo/` (sha256 `83d80954cde22ed16509aec22baeb36545d8cf486dc409755f325d5585004cce`).
It is an older (2019) amd64 tool (`credo`, `credo_util`, `libPhyCredo.so`) for the
"BE" family with `BE.fw.v2.19.15{,.B0}.bin`, 7280CR3/7800R3 configs. Not Owl, not
arm64, not useful for us (V from `file`/control).

### Tests
None run: no aarch64 ELF exists to test. No qemu/glibc/export comparison done.

### Usability on the switch
- The only 1.2.12 Owl code remains the amd64 `libsai.so.1`/`libowl.so`; it cannot be
  dlopen'd natively on aarch64. (V)
- The amd64 libs need glibc >= 2.34-class symbols (Arista package declares
  `libc6 >= 2.34`; I for the SAI debs, check `objdump -T` if pursued) so the
  vendor chroot's glibc 2.25 would not suffice even if they were arm64. (I)
- Remaining routes: (1) x86 `libsai` under qemu-user with an MDIO shim (heavy),
  (2) read the DWARF in amd64 `libsai.so.1` and reimplement on top of our arm64
  `libdriver_164_u.so`/`libowl.so`, (3) ask Credo/Celestica for an aarch64 SDK. (I)
- Firmware 1.94.11 is arch-neutral data and can be tried independently of any lib.
  (I)
