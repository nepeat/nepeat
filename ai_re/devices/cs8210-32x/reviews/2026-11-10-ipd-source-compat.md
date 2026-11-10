# 2026-11-10 — can the published `ipd` source serve the old SDK userspace on the CS8210?

Offline analysis only; nothing touched the switch, nothing was loaded or run.

- New source: `https://github.com/Marvell-switching/mrvl-teralynx`, commit
  `53d8dcf` ("Add Trixie support"), `drivers/ipd/`, `IPD_VERSION_STRING
  "0.16.2.0.0.0"`, GPLv2 option. 33 kLOC including generated register headers.
- Old side: vendor `ipd.ko` 0.12.41.1.0.1 (sha256 `a1867705…0dc547`, identical
  in SDK 0.1.2 and the installed 0.1.7), `libifcs.so` / `innovium.user` from
  the same SDK, and the stock reference run in
  `evidence/stock-sdk-run-2026-11-10/`.

Marking: **V** = verified (read in the new source, read from the old module's
DWARF/disassembly, or produced by a compile), **L** = also seen in the live
stock run, **I** = inferred.

## Verdict

**Not compatible as-is; compatible with one patch of four small changes** (one
module parameter `legacy_abi=1` gating all of them). The patch is written and
compile-tested, not run:
`reviews/ipd-source-compat/0001-ipd-legacy_abi-for-0.12.x-userspace.patch`
(682 lines of diff: one new header, `ipd_main.c`).

What already matches (**V**): ioctl magic `'I'` and all 25 command numbers the
old library uses; header version (both sides 1); mmap semantics (offset =
physical address); PCI ID 1d98:1b58 with rev 0x10 known as "B0"; the Teralynx 7
packet-header code path (`*_v0`, 12-byte header, 32-byte debug header);
netdev naming `inno_<inst>_<sysport:04d>`; interrupt mode parameter and
default; pool size and the way its offsets are reported.

What breaks without the patch:

1. **11 of the ioctl argument structures grew** (new `*_pa` fields, 10-word
   interrupt masks, 44-byte LDH area, 20-byte system headers). The kernel
   copies the new size and reads fields at new offsets, so the old library gets
   wrong addresses (QUERY: `pool_baddr` would read as `bar2` = 0), and on
   RUPT_WAIT / GET_NETDEV the kernel writes 8 / 64 bytes past the caller's
   struct.
2. **`open()` no longer initialises the device.** The init sequence the old
   module ran in `inno_open` now lives behind two new ioctls (`IPD_HW_INIT`
   nr 34, `IPD_DMA_INIT` nr 32) that the old library never issues. Without it
   there is no pool, no interrupt setup and no NAPI context.
3. **Device node** is `/dev/ipd0` (per-device cdev, dynamic major) instead of
   `/dev/ipd` (major 101).
4. **`IPD_INFO_NODES`** no longer fills `num_nodes` and reports only the
   device behind the opened file.

The source **builds unmodified against Linux 7.2.11 arm64** (`defconfig`,
GCC 15.3 cross): no errors, only `-Wmissing-prototypes` noise; every imported
symbol is exported by that tree. The patched tree builds the same way. 7.3.3
itself and the NixOS kernel config were not tried.

## 1. Version / magic checks

| | old 0.12.41 | new 0.16.2 |
| --- | --- | --- |
| ioctl magic | `'I'` (0x49); kernel ignores it (V) | `IFCS_IPD_MAGIC 'I'`; kernel uses `_IOC_NR(cmd)` only (V) |
| `hdr.version` expected | 1 (V: `cmp w0,#1` at 0x9120) | `IFCS_PCIE_IOCTL_VERSION 1` (V) |
| what `libifcs.so` sends | requests `0xc00849NN` (`_IOWR('I', nr, void *)`), `0x80084901` for nr 1 (V: 37 call sites). `version = 1` is I from the kernel check plus the working live run, I did not trace the store. | same macros |
| `hdr.instance` check | `> inno_num_devices` -> `-EINVAL` | same |
| driver version string | returned in QUERY, `"0.12.41.1.0.1"` | `"0.16.2.0.0.0"` |

No handshake rejects the old library. Whether the old library compares the
returned version string was not checked (I: unlikely to be fatal; the string
is 32 bytes in both).

## 2. ioctl ABI differences

Sizes: old from the module's DWARF, new from compiling `inno_ioctl.h`
(LP64). "nr" = `_IOC_NR`. All numbers 1-26 are unchanged; 27-40 are new.

| nr | command | old size | new size | difference | effect on the old library |
| ---: | --- | ---: | ---: | --- | --- |
| 1 | INFO_NODES | 76 | 76 | layout same; new kernel does not set `num_nodes`, fills `node_info[0]` only | probe sees 0 nodes (I) |
| 2 | QUERY_NODE | 112 | 136 | `bar2`, `bar2_size` inserted at 0x20; `pool_baddr` 0x20 -> 0x30; `pool_paddr` added at 0x38; everything after shifts by 0x18 | wrong pool address and offsets |
| 3 | RUPT_MASK | 44 | 52 | `mask[8]` -> `mask[10]` | works by accident (prefix-compatible, kernel over-reads 8 bytes) |
| 4 | RUPT_WAIT | 64 | 72 | `rupts[8]` -> `rupts[10]` | kernel writes 8 bytes past the user struct |
| 5/6 | READ_MEM / WRITE_MEM | 8 | 8 | none (not used by the old library) | — |
| 7/8/23/24 | RING_ALLOC/FREE/ENABLE/DISABLE | 48 | 64 | `ring_pa`, `wb_pa` inserted at 0x18; `count` 0x18 -> 0x28, `flags` 0x1c -> 0x2c, `num` 0x20 -> 0x30, `idx_offset` 0x24 -> 0x34, `cidx/pidx` 0x28 -> 0x38 | broken |
| 9/10 | HRR_ALLOC/FREE | 32 | 40 | `hrr_pa` inserted at 0x10; `size/count/descriptors` 0x10 -> 0x18, `hrr_pidx_offset` 0x18 -> 0x20 | broken |
| 11/12 | LEARN_ALLOC/FREE | 40 | 56 | `learn_pa` at 0x10, `wb_ba` 0x10 -> 0x18, `wb_pa` at 0x20; rest shifts by 0x10 | broken |
| 13 | TX_SEND | 64 | 80 | `ldh[24]` -> `ldh[44]`; `num_ldh` 0x34 -> 0x48, `flags` 0x38 -> 0x4c | broken |
| 14 | UNPIN_PAGE | 12 | 12 | none | — |
| 15/16 | CREATE/DELETE_NETDEV | 48 | 52 | `intf_name[16]` -> `[17]`; new flag bit 2 = CPU port | prefix-compatible, kernel over-reads 4 bytes |
| 17/18 | RING_PAGE_ALLOC/FREE | 32 | 40 | `buf_pa` inserted at 0x10; `ring_type` 0x10 -> 0x18, `ring_num` 0x14 -> 0x1c, `idx` 0x18 -> 0x20 | broken |
| 19/20/21 | PIC_STATUS_ALLOC/FREE/DUMP | 24 | 32 | `wb_pa` inserted at 0x10; `count` 0x10 -> 0x18 | broken |
| 22 | (old: none) DEBUG_EVT_THREAD_TIMER | — | 32 | new | not used |
| 25/26 | GET/SET_NETDEV | 76 | 140 | union grows: `syshdr1[24]` -> `[40]`, `syshdr1_cnt` 0x24 -> 0x34, `syshdr1_abp[24]` -> `[40]` at 0x35, new `syshdr1_ptp[20]` at 0x5d, `syshdr2[12]@0x3d` -> `[20]@0x71`; other union members keep offset 0x0c; three new ids (7-9) | SYSHDR set stores garbage headers; GET writes 64 bytes past the user struct |
| 27-40 | netlink, params, DMA_INIT, FLUSH_RECOVER_DMA, HW_INIT, USER_CONFIG_*, OVERRIDE_FLOW_CONTROL, CLEANUP_RESOURCES, SFLOW | — | | new | never issued; see section 4 for 32/34 |

New definitions, for the record (`inno_ioctl.h`, V):

```c
typedef struct { inno_ioctl_hdr_t hdr; uint64_t serial_num, bar0, bar0_size, bar2, bar2_size,
                 pool_baddr, pool_paddr; uint16_t vendor_id, device_id; uint8_t rev_id, bus_num,
                 dev_num, fn_num; uint32_t pool_size, rupt_wb_size, rupt_wb_offset, sync_blk_size,
                 sync_blk_offset, sync_wb_size, sync_wb_offset, dma_cache_align;
                 uint8_t version[32]; } inno_ioctl_query_t;                      /* 136 */
typedef struct { inno_ioctl_hdr_t hdr; uint64_t ring, wb, ring_pa, wb_pa; uint16_t count, vector;
                 inno_ring_flags_t flags; uint8_t num; uint32_t idx_offset;
                 uint16_t cidx, pidx; } inno_ioctl_ring_t;                       /* 64 */
#define RUPT_MASK_WORDS_MAX 10   /* was 8 */
#define LDH_MAX_HEADERS_SIZE 44  /* was 24 */
#define SYSHDR_SIZE 20           /* was 12 */
```

The old layouts are in the patch as `inno_ioctl_*_v0_t` with `static_assert`s
on every size and the shifted offsets (they hold: the patched module compiles).

Return codes are unchanged where I compared them (`-EACCES` on user-copy
failure, `-EINVAL` on version/instance).

## 3. Device node

| | old | new |
| --- | --- | --- |
| region | `register_chrdev_region(MKDEV(101,0), 8, "ipd")`, dynamic fallback (V, L: major 101) | `alloc_chrdev_region(&dev, 0, 8, "ipd")`, always dynamic (V) |
| cdev | one cdev for 8 minors, `iminor()` indexes `inno_instances[]` | one cdev per device, minor from an IDR, `container_of(inode->i_cdev)` |
| node | `device_create(.., "ipd")` once at module init -> `/dev/ipd` | `device_create(.., &pdev->dev, .., "ipd%d", minor)` in probe -> `/dev/ipd0` |
| open exclusivity | module-global flag, second writer `-EEXIST` | per-device `dev_opened`, same `-EEXIST` |
| extra fops | — | `.read` (`ipd_read`) |

`libifcs.so` opens the literal string `"/dev/ipd"` (V, `.rodata` 0x1863550,
two call sites, `O_RDWR`). The major number does not matter to it. Either of:

- no kernel change: `ln -s ipd0 /dev/ipd` in the chroot's `/dev`, or a udev
  rule `KERNEL=="ipd0", SYMLINK+="ipd"`;
- the patch names minor 0 `ipd` when `legacy_abi=1`.

## 4. open / init flow and mmap

**mmap is the same** (V both sides): `phys = vm_pgoff << PAGE_SHIFT`, no range
check, `VM_IO | VM_DONTEXPAND | VM_DONTDUMP`, `pgprot_noncached` on non-x86,
`remap_pfn_range` page by page (new code uses `vm_flags_set` from 6.3). The
new comment calls the offset a "kernel virtual addr" but the code treats it as
a physical address, as the old module did.

The new ABI returns **two** addresses per buffer (`ring` = DMA address for the
hardware, `ring_pa` = CPU physical address for mmap, from
`inno_get_phys_addr()`: `virt_to_phys()` when the device is behind an IOMMU,
else the DMA address). The old ABI has one field that the old library uses for
both. That is only correct when DMA address == physical address, which is the
case on this board as currently described (no `iommus`/`dma-ranges` on the
`pcie-al` nodes in `alpine-v3-cs8210.dts`; L: the stock run mmapped the same
values the driver programmed). The patch refuses legacy mode with
`-EOPNOTSUPP` when `device_iommu_mapped()`.

**Init moved out of `open`.** Old `inno_open` (write mode) ran, in order:
endian-swap clear, clear interrupt causes, `inno_napi_init`, swap enable,
`inno_syncmode_config` (allocates the 0x2000 pool, programs `HSR_ADR`,
`SYNC_HSN_ADR_0`, `INTR_WB_*`), `inno_intr_init`, MSI workaround, IAC mask,
then global DMA / SBUF setup (V, old disassembly 0xc640-0xcd10). New
`inno_open` only sets `dev_opened`. The same steps are now
`inno_hw_init()` (ioctl 34) followed by `inno_dma_init()` (ioctl 32), V.
Two differences inside them:

- `inno_hw_init` first calls `inno_cpu_block_init()`, a chip CPU-block
  memory-init / reset sequence guarded by a scratchpad pattern in
  `SYNC_HSN_ADR_LO_3` (`0xAABBCCDD`). The old kernel module has nothing like
  it; the old `libifcs.so` exports `ii_is_cpu_block_init_done` and
  `ii_cpu_block_init_scratchpad_set` (V, symbol names only), so the old
  library does this itself (I). The patch skips the kernel-side call in legacy
  mode to reproduce the old division of labour. If userspace turns out to rely
  on the kernel doing it, drop that hunk: the scratchpad makes it idempotent.
- `DMA_GLOBAL`: old wrote 0x39f with `cacheline_size_f = cache_align & 3`
  (0 for its fixed `cache_align` of 0x80) and all three cacheline-split bits
  set; new writes `cacheline_size_f = cache_align >> 6`, only
  `en_dm_cacheline_split_f`, and takes `cache_align` from
  `dma_get_cache_alignment()` (64 on a Cortex-A72 with 7.x, I) instead of a
  constant 128. Pool offsets therefore change (0x40-stride instead of 0x80)
  but the library reads them from QUERY / RING_ALLOC / HRR_ALLOC, so that
  should be transparent (I). The register value difference is a behavioural
  change of the newer driver on the same chip; untested here.

`release`: new `inno_close` calls `inno_cleanup_resources()` unless
`boottype=1` (fast boot), which frees rings, netdevs and interrupts like the
old close. The stock flow's open -> close -> open (python shell first, then
`innovium.user`; L in dmesg) therefore re-runs init on the second open in
legacy mode.

## 5. Chip support and packet headers

- ID table (V): `{1d98:1b58}` (`INNO_TERALYNX_PCI_DEVICE_ID`) plus Marvell
  0x6000 (TL10), 0x6401 (TL12), 0x6800 (T100). Revisions named for 0x1b58:
  A0 = 0x00, **B0 = 0x10** (ours). The only revision-dependent code for this
  ID is the MSI workaround, applied to A0 only, exactly as in the old module
  (old: `device_id != 0x1b58 || rev_id == 0`).
- Header version selection is by device ID in `inno_napi_init` (V): 0x1b58 ->
  `inno_unpack_ldh_header_v0`, `inno_debug_hdr_present_v0`,
  `inno_unpack_ext_hdrs_v0`, `inno_populate_inno_hdr_v0`, `inno_get_ssp_v0`,
  `inno_vf2_queue_set_v0` (`inno_ldh_v0.c`, `ldh_structs_ipd_v0.h`,
  `inno_cmipd_v0.*`). v2 = TL10, v3/v4 = TL12/T100 (I for which of the two).
- Lengths (V): `INNO_TERALYNX_CHIP_HDR_LEN 12`, `..._DBG_HDR_LEN 32`. The old
  module strips 12 bytes, or 0x2c = 44 when bit 7 of byte 0 is set (old
  disassembly 0x2988-0x29a0): same numbers.
- RX fields (V both): source system port from the vf0 header (`ssp_f`, 11
  bits, bytes 4-5 in the old code), `cpu_queue_num_f`, `cpu_metadata_f`;
  frames with `vf_type` other than the supported ones are dropped. I did not
  diff `ldh_structs_ipd_v0.h` field by field against the old DWARF (the old
  DWARF only carries `ldh_fixed_format_t` and `ldh_vf0_t`); the field names
  that exist in both agree.
- TX (V): bypass path sends `chip_hdr_len` (12) bytes of the per-sysport
  `syshdr2`, lookup path `12 * syshdr1_cnt` bytes of `syshdr1`/`syshdr1_abp`,
  as in the old module. Storage stride changed from 12/24 to 20/40 bytes per
  sysport, kernel-internal; the patch's SET_NETDEV conversion copies the old
  12/24-byte blobs into the start of the new slots.
- The info header (`inno_info_header_t`, single-interface mode) grew new
  fields in place of the old `rsvd[8]`; it is still 24 bytes (I from the
  field list, not compiled). Only relevant if the SDK is configured for a
  single netdev, which the Celestica config is not (`netdev: auto_create`).

## 6. Netdevs and the knet flow

Old flow (V/L): `common_*_nd.yaml` has `netdev: - auto_create: "yes"` and
`netdev: "true"` on TX rings 1-3 and the RX rings; after start,
`cls_shell "diagtest knet config -a <mac>"` renames `inno_0_0001..0032` to
`fpp1..fpp32`.

| aspect | old | new |
| --- | --- | --- |
| default name | `inno_%1.1d_%4.4d` (instance, sysport); `inno%d` in single-interface mode | identical (V, `inno_enet.c:2053-2055`) |
| name assign type / alloc | `alloc_netdev_mqs(16, name, NET_NAME_PREDICTABLE, ether_setup, 1, 1)` | `alloc_netdev(sizeof(inno_enet_adapter_t), name, NET_NAME_PREDICTABLE, ether_setup)` |
| MAC | `eth_mac_addr` from the ioctl, random if invalid | same logic (I, not diffed line by line) |
| MTU | `INNO_ND_MTU` writes `dev->mtu` directly (9462 live) | same; additionally `min_mtu = ETH_MIN_MTU`, `max_mtu = ETH_MAX_MTU` so later `ip link set mtu` works |
| link state | `INNO_ND_LINKSTATE` -> `netif_carrier_on/off` | same |
| up/down | `INNO_ND_IFSTATE` -> `dev_open`/`dev_close` under RTNL | same |
| carrier at create | off | same (I) |
| new | — | CPU-port flag, PTP, sFlow/genetlink, ethtool-less still |

Renaming by userspace needs nothing from the driver. Nothing in the old
flow depends on a removed feature.

## 7. Interrupts and module parameters

| | old | new |
| --- | --- | --- |
| `inno_intr` | 1 best effort (default), 2 MSI-X, 3 MSI, 4 INTx | identical (V) |
| `loglevel` | 0-7, default 2 | same parameter |
| `boottype` | — | new, 0 cold (default) / 1 fast |
| MSI-X vectors | exactly 7 (`pci_enable_msix_range(.., 7, 7)`): napi, async, tx, rx, err, lrn, pic (L) | exactly 8 (`NUM_MSIX_VECS`): the same seven plus `MSIX_VECTOR_RX_MCU0` = 7 |
| interrupt words | 8 | 8 for 0x1b58 (`rupt_mask_words`), 10 for TL10 |
| vector limit in RUPT_MASK/WAIT | `> 6` -> `-EINVAL` | `>= 8` -> `-EINVAL` |

The chip advertises 32 MSI-X vectors (L, `lspci`), so asking for 8 is fine. The
old library only uses vectors 1-6.

## 8. Building and running on mainline 7.x arm64

Verified by compiling (scratch build, `linux-7.2.11`, `ARCH=arm64 defconfig`,
`modules_prepare`, then `make M=`):

- unmodified source: all 11 objects and `ipd.ko` build; warnings are
  `-Wmissing-prototypes` and one `-Wold-style-declaration`. `modinfo`:
  `vermagic 7.2.11 SMP preempt mod_unload aarch64`, parameters `inno_intr`,
  `boottype`, `loglevel`.
- every undefined symbol of `ipd.o` (131) has an `EXPORT_SYMBOL*` in the tree
  (`get_user_pages_fast` and `__vma_start_write` are GPL-only; the module is
  GPL).
- patched source: builds with the same warnings.

Kernel-version handling in the source (V): conditionals up to 6.4
(`class_create`), 6.3 (`vm_flags_set`), 6.1, 5.18 (PCI DMA wrappers), and
`ipd_compat.h` replaces `strncpy` from 6.19 on. So it is maintained for
current kernels; nothing needed changing for 7.2.

Not verified, to watch at run time:

- **No x86-only code paths block arm64**, but arm64 is the less-travelled
  path: the mmap handler applies `pgprot_noncached` to RAM pages on non-x86
  (same as the old module did on this very board, L).
- **IOMMU / DMA offset**: see section 4. Legacy mode needs identity DMA.
- **`cache_align` 64 vs the old fixed 128** and the `DMA_GLOBAL` difference,
  section 4.
- **SError exposure is unchanged**: userspace still gets all of BAR0 mapped,
  so an unacknowledged read by the vendor binary still panics mainline. The
  driver cannot help with that. Probe itself does not touch BAR0 for 0x1b58
  (V: the only probe-time `REG32` uses are TL10/TL12/T100 branches) and calls
  `pci_set_master` first and `pci_enable_device` last, like the old one; the
  endpoint's memory decode must be on before the first open (it showed
  `COMMAND=0006` under mainline, so it is).
- The dummy NAPI netdev is an unregistered `alloc_netdev` device; accepted by
  7.2 headers, run-time behaviour not checked.
- 4K pages: `POOL_SIZE` is `2 * PAGE_SIZE` = 0x2000, matching the old pool (L:
  0x2000 mapping at 0xaf11a000). Ring pages are order-0 with a hard-coded
  0x1000 map length in places; correct for 4K.

## 9. The patch

File: `reviews/ipd-source-compat/0001-ipd-legacy_abi-for-0.12.x-userspace.patch`
(apply with `-p1` in the repository root). Compile-tested only. Load with
`insmod ipd.ko legacy_abi=1`; with the parameter at its default 0 the module
behaves exactly as upstream.

Hunk 1, parameter (`ipd_main.c`):

```c
static bool ipd_legacy_abi;
module_param_named(legacy_abi, ipd_legacy_abi, bool, 0444);
MODULE_PARM_DESC(legacy_abi, "1 = ipd 0.12.x userspace ABI (default 0)");
```

Hunk 2, legacy structures: new `inno_ioctl_legacy.h` with the eleven
`inno_ioctl_*_v0_t` layouts and `static_assert`s (sizes 112, 44, 64, 48, 32,
40, 24, 64, 48, 76, 32).

Hunk 3, translation (`inno_ioctl_legacy()`, about 330 lines, inserted before
`inno_ioctl`) and its hook:

```c
    idev = f->private_data;
    ...
    if (ipd_legacy_abi) {
        long lret = inno_ioctl_legacy(idev, cmd, arg);

        if (lret != 1)          /* 1 = layout unchanged, use the normal switch */
            return lret;
    }
    switch (_IOC_NR(cmd)) {
```

Each case copies the old struct in, checks instance/version as the normal
path does, fills the current struct, calls the existing helper
(`inno_rupt_mask`, `inno_rupt_wait`, `inno_alloc_ring`, `inno_free_ring`,
`inno_enable/disable_queue_*`, `inno_alloc_hrr`, `inno_alloc_learn`,
`inno_tx_send`, `inno_netdev_create/delete/set/get`,
`inno_ring_page_alloc/free`, `inno_pic_st_*`), copies results back into the
old struct and copies out the old size. INFO_NODES sets `num_nodes` and all
probed nodes; QUERY_NODE is re-implemented for the old layout. The single
address field of each old struct gets the DMA address (`ring`, `wb`, `hrr`,
`learn_ba`, `wb_ba`, `buf_ba`, `pool_baddr`), as the old module did.

Hunk 4, init at open (`inno_open`) and no kernel CPU-block init
(`inno_hw_init`):

```c
    if (ipd_legacy_abi) {
        int rc;

        if (device_iommu_mapped(&idev->pdev->dev))
            return -EOPNOTSUPP;
        rc = inno_hw_init(idev);
        if (rc == 0)
            rc = inno_dma_init(idev);
        if (rc)
            return rc;
    }
    idev->dev_opened = 1;
```

```c
    if (!ipd_legacy_abi) {
        rc = inno_cpu_block_init(idev);
        ...
    }
```

Hunk 5, device node (`inno_probe`):

```c
    if (ipd_legacy_abi && minor == 0)
        idev->dev_node = device_create(inno_cl, &pdev->dev, devno, NULL, IPD_DRIVER_NAME);
    else
        idev->dev_node = device_create(inno_cl, &pdev->dev, devno, NULL, IPD_DRIVER_NAME "%d", minor);
```

Known gaps in the patch: `IPD_GET_NETDEV` with id `INNO_ND_SYSHDR` returns the
caller's buffer unchanged (I did not check whether the old library ever reads
system headers back); ids above `INNO_ND_RING` are rejected with -1 as the old
module did; a failed legacy open leaves `private_data` set, as the old module
also did.

An alternative that leaves the kernel module untouched is an `LD_PRELOAD`
shim in the SDK chroot that wraps `ioctl()` and `open()` and does the same
translation in userspace (plus issuing nr 34 and 32 after open). It needs a
glibc-2.17-compatible build and cannot skip the kernel-side CPU-block init, so
I prefer the module parameter.

## 10. What I did not establish

- Nothing was executed against hardware: the claim is "builds, and the ABI
  now lines up on paper".
- Whether the old `libifcs.so` really performs the CPU-block init itself
  (symbol names only), and whether it tolerates `dma_cache_align = 64`.
- Field-by-field equality of the v0 LDH structures, the checksum-trailer
  handling in `inno_tx_send`/`inno_enet_tx`, and ring-index conventions
  between 0.12 and 0.16: both versions share function names, log strings and
  the register sequences I spot-checked (queue enable/disable, HRR, SBUF
  setup), but I did not diff the whole data path.
- Whether a newer `libifcs`/SDK matching 0.16.2 exists for Teralynx 7; with
  one, no patch would be needed.
- Build against the actual NixOS 7.3.3 kernel configuration.

## Leftovers from the superseded disassembly work

Still useful as the record of the old ABI and kept in `reviews/ipd-skeleton/`:
`ioctl-dispatcher-notes.md` (all 26 old handlers, from disassembly),
`teralynx_pci_regs.h` (2277 BAR0 register offsets from the old module's
DWARF; the new tree's `pci_common_ipd.h` supersedes it) and
`per-queue-register-tables.txt`. The from-scratch port spec and C skeleton
were stopped before being written.
