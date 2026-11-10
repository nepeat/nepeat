# inno_ioctl (ipd.ko, 0x9000..0xc42f, ipd_main.c:2199+) — behavioural spec

All addresses are .text addresses from c.dis. "arg" = the on-stack copy of the user struct at
`x29+0x120` (=x29+288; so user offset = stack displacement - 288). `idev` = `filp->private_data`
(x23, loaded at 0x9038 `ldr x23,[x22,#208]`). Claims are VERIFIED (read from disassembly/DWARF)
unless tagged INFERRED.

## 0. Common machinery

### 0.1 Dispatch (VERIFIED)
```
9048 and w0, w19, #0xff ; 904c sub w0,w0,#1 ; 9050 cmp w0,#0x19 ; 9054 b.ls 9090
9090.. ldrh w0,[.rodata+0x4d0, w0*2] ; adr x1,90a8 ; add x0,x1,w0,sxth #2 ; br x0
```
Only `cmd & 0xff` is used (no check of _IOC_TYPE/_IOC_DIR/_IOC_SIZE). nr 0, nr > 26, and nr 22 go to
0x9058: optional `IPD-WARN: Bad ioctl. cmd: %d` (loglevel > 1) and return **-EINVAL (-22)**.
I re-extracted the table from `.rodata+0x4d0`; it matches `ipd-abi.json:dispatch_low_byte` exactly.

Hint for the full cmd encoding (INFERRED): the entry debug printk at 0xbce4
(`IPD-VERBOSE: Ioctl $%p %x %p %p`, loglevel > 6) passes the constant **0x80084901** as its first
vararg = `_IOR('I'(0x49), 1, 8-byte type)`. So the vendor header very likely defines commands as
`_IOx(0x49, nr, <pointer-sized type>)`; magic 0x49. The kernel does not care.

### 0.2 Table

| nr | handler | src line | proposed name | arg struct | bytes in | bytes out | dir |
|---|---|---|---|---|---|---|---|
| 1 | 0xa10c | 2229 | INNO_IOCTL_NODES | inno_ioctl_nodes_t | 76 | 76 | RW |
| 2 | 0x9f28 | 2256 | INNO_IOCTL_QUERY | inno_ioctl_query_t | 112 | 112 | RW |
| 3 | 0xa304 | 2317 | INNO_IOCTL_RUPT_MASK | inno_ioctl_rupt_mask_t | 44 | 0 | W |
| 4 | 0x9b90 | 2336 | INNO_IOCTL_RUPT_WAIT | inno_ioctl_rupt_wait_t | 64 | 64 | RW |
| 5 | 0x9b30 | 2359 | INNO_IOCTL_READ (reg/pool word read) | raw 8 bytes {u32 off; u32 val} (sizeof hdr) | 8 | 8 | RW |
| 6 | 0x9ad0 | 2385 | INNO_IOCTL_WRITE (BAR0 word write) | raw 8 bytes {u32 off; u32 val} | 8 | 0 | W |
| 7 | 0x9a30 | 2393 | INNO_IOCTL_ALLOC_RING | inno_ioctl_ring_t | 48 | 48 | RW |
| 8 | 0x9984 | 2415 | INNO_IOCTL_FREE_RING | inno_ioctl_ring_t | 48 | 48 | RW |
| 9 | 0x9e90 | 2494 | INNO_IOCTL_ALLOC_HRR | inno_ioctl_hrr_t | 32 | 32 | RW |
| 10 | 0x9df0 | 2518 | INNO_IOCTL_FREE_HRR | inno_ioctl_hrr_t | 32 | 32 | RW |
| 11 | 0xa294 | 2540 | INNO_IOCTL_ALLOC_LEARN | inno_ioctl_learn_t | 40 | 40 | RW |
| 12 | 0x9d50 | 2562 | INNO_IOCTL_FREE_LEARN | inno_ioctl_learn_t | 40 | 40 | RW |
| 13 | 0x98e0 | 2584 | INNO_IOCTL_TX_SEND | inno_ioctl_tx_send_t | 64 | 64 | RW |
| 14 | 0x9748 | 2606 | INNO_IOCTL_UNPIN_PAGES | inno_ioctl_unpin_pages_t | 12 | 12 | RW |
| 15 | 0xa214 | 2628 | INNO_IOCTL_NETDEV_CREATE | inno_ioctl_netdev_t | 48 | 0 | W |
| 16 | 0x96c8 | 2647 | INNO_IOCTL_NETDEV_DELETE | inno_ioctl_netdev_t | 48 | 0 | W |
| 17 | 0x9614 | 2709 | INNO_IOCTL_RING_PAGE_ALLOC | inno_ioctl_ring_page_alloc_t | 32 | 32 | RW |
| 18 | 0x9590 | 2735 | INNO_IOCTL_RING_PAGE_FREE | inno_ioctl_ring_page_alloc_t | 32 | 0 | W |
| 19 | 0x9394 | 2754 | INNO_IOCTL_PIC_ST_ALLOC | inno_ioctl_pic_st_t | 24 | 24 | RW |
| 20 | 0x922c | 2776 | INNO_IOCTL_PIC_ST_FREE | inno_ioctl_pic_st_t | 24 | 24 | RW |
| 21 | 0x94c4 | 2798 | INNO_IOCTL_PIC_ST_DUMP | inno_ioctl_pic_st_t | 24 | 24 | RW |
| 22 | 0x9058 | 2820 | (unused → default) | — | — | — | -EINVAL |
| 23 | 0x9450 | 2438 | INNO_IOCTL_ENABLE_QUEUE (ring enable) | inno_ioctl_ring_t | 48 | 48 | RW |
| 24 | 0x9178 | 2466 | INNO_IOCTL_DISABLE_QUEUE (ring disable) | inno_ioctl_ring_t | 48 | 48 | RW |
| 25 | 0x90d4 | 2684 | INNO_IOCTL_NETDEV_GET (netdev info get) | inno_ioctl_netdev_info_t | 76 | 76 | RW |
| 26 | 0x90a8 | 2666 | INNO_IOCTL_NETDEV_SET (netdev info set) | inno_ioctl_netdev_info_t | 76 | 0 | W |

Names are INFERRED from struct types, inlined helper names (DWARF) and callee names; the sizes and
directions are VERIFIED. Source-line order shows the switch was written in the order
1,2,3,4,5,6,7,8,23,24,9,10,11,12,13,14,15,16,26,25,17,18,19,20,21 (enable/disable sit right after
free-ring in the source; nr 22 has no case).

### 0.3 User copy and header validation (identical everywhere unless noted)
* `copy_from_user`/`copy_to_user` are open-coded as `__range_ok(arg, size)` then
  `__arch_copy_{from,to}_user`. **Any failure (access_ok false OR non-zero residue) returns -EACCES
  (-13), not -EFAULT**: `90cc mov x19,#-13`.
* After copy-in (line order in source: instance first, then version):
  ```
  910c adrp x0,{.bss+0x10}        ; inno_num_devices
  9110 ldrb w1,[x29,#290]         ; hdr.instance
  9118 cmp w1,w0 ; 911c b.hi 9064 ; -EINVAL if instance >  inno_num_devices   (note: '>' not '>=')
  9120 ldrh w0,[x29,#288] ; cmp w0,#1 ; b.ne 9064   ; -EINVAL if hdr.version != 1
  ```
  **Expected hdr.version = 1. Wrong version or instance → -EINVAL (-22).** unused1/unused2 never read.
* **The device is always `filp->private_data`**; hdr.instance is only range-checked. Sole exception:
  QUERY (nr 2), which uses `&inno_instances[hdr.instance]` or a serial/vendor/device lookup and never
  touches private_data. (With `>` the value instance == inno_num_devices passes; for QUERY this indexes
  a not-probed (zeroed) or, for 8, out-of-array slot.)
* Exceptions to validation: NODES checks only version; nr 5/6 have no header at all.
* Callee error: every `cbnz w0, a2fc` → `a2fc sxtw x19,w0` → the callee's int return value is returned
  as-is and **nothing is copied back**.
* Success return is 0 for every command.
* No lock is taken by the dispatcher itself; only rupt_wait takes `idev->rupt_lock`.

## 1. nr 1 — NODES (0xa10c)
1. copy_from_user 76. Only `hdr.version == 1` is checked (a148..a150) → else -EINVAL. No instance check.
2. `arg.num_nodes (u8 @+8) = (u8)inno_num_devices` (a15c/a160).
3. For i in 0..inno_num_devices-1 (loop a194..a1d0, source ptr = &inno_instances[i].vendor_id =
   .bss+0x10+0x801c + i*0xae78, dest = arg+0x0a + 8*i):
   `node_info[i].vendor_id = idev[i].vendor_id (+0x8014)`, `.device_id = +0x8016`,
   `.rev_id = +0x8018`, `.bus_num = +0x8028`, `.dev_num = +0x8029`, `.fn_num = +0x802a`.
   Entries >= num_devices keep whatever the user passed in.
4. copy_to_user 76 → 0 / -EACCES.

## 2. nr 2 — QUERY (0x9f28)
1. copy_from_user 112; instance > inno_num_devices → -EINVAL; version != 1 → -EINVAL.
2. Device selection:
   ```
   9f88 ldr x3,[x29,#296]   ; arg.serial_num
   9f8c cbz x3, a508        ; serial_num == 0  -> use hdr.instance
   9f90 ldrh w6,[x29,#330]  ; arg.device_id
   9f94 cbz w6, a508        ; device_id == 0   -> use hdr.instance
   ```
   * serial_num == 0 **or** device_id == 0: `qdev = &inno_instances[hdr.instance]` (a508..a51c).
   * otherwise: linear search i = 0..7 over all 8 slots of inno_instances (not just num_devices) for
     `serial_num == arg.serial_num && vendor_id == arg.vendor_id && device_id == arg.device_id`
     (9fc0..9fe0). Not found → -EINVAL. Found → `arg.hdr.instance = i` (9ff0 strb) and qdev = that slot.
   (In the instance path hdr.instance is left as given.)
3. Fill (ca = qdev->cache_align, +0x802c; ALIGN(x) = (x + ca - 1) & -ca, all 32-bit arithmetic):

| field (offset) | value |
|---|---|
| serial_num (+0x08) | qdev->serial_num (+0x8020) |
| bar0 (+0x10) | qdev->bar0_ba (+0x8038) (the bus/phys address, not the ioremap) |
| bar0_size (+0x18) | (u64) qdev->bar0_size (+0x8040, u32) |
| pool_baddr (+0x20) | qdev->pool_ba (+0x8a38) |
| vendor_id (+0x28), device_id (+0x2a), rev_id (+0x2c) | +0x8014, +0x8016, +0x8018 |
| bus_num (+0x2d), dev_num (+0x2e), fn_num (+0x2f) | +0x8028, +0x8029, +0x802a |
| pool_size (+0x30) | constant **0x2000** |
| rupt_wb_size (+0x34) | constant **0x20** (32) |
| rupt_wb_offset (+0x38) | sync_blk_offset + 2*ALIGN(8) + ALIGN(0x1000) |
| sync_blk_size (+0x3c) | constant **0x1000** |
| sync_blk_offset (+0x40) | sync_wb_offset + ALIGN(0x104) |
| sync_wb_size (+0x44) | constant **0x104** (260) |
| sync_wb_offset (+0x48) | ALIGN(4) * 13 |
| dma_cache_align (+0x4c) | ca |
| version[32] (+0x50) | `strncpy(arg.version, "0.12.41.1.0.1", 32)` (zero padded) |

   Evidence:
   ```
   a018 ldr w2,[x6,#44]            ; ca
   a024 neg w5,w2 ; a030 add w3,w2,#3 ; a034 and w3,w3,w5 ; a044 mul w3,w3,w8(=13) ; a058 str w3,[x29,#360]  ; sync_wb_offset
   a04c add w4,w2,#0x103 ; a054 and w4,w4,w5 ; a05c add w4,w4,w3 ; a084 str w4,[x29,#352]                   ; sync_blk_offset
   a06c add w3,w2,#7 ; a070 and w3,w3,w5 ; a088 add w4,w4,w3,lsl #1
   a07c add w2,w2,#0xfff ; a08c and w3,w2,w5 ; a098 add w3,w4,w3 ; a0ac str w3,[x29,#344]                   ; rupt_wb_offset
   ```
   Worked values: ca=0x80 (the default per the peer analysing inno_open) → sync_wb_offset=0x680,
   sync_blk_offset=0x880, rupt_wb_offset=0x1980 (ends 0x19a0 < 0x2000). ca=0x40 → 0x340, 0x480, 0x1500.
   The "13" = 7 TX + 6 RX per-ring index-writeback slots of ALIGN(4) each (see ALLOC_RING idx_offset).
   Implied pool layout (INFERRED from the arithmetic): `[13 ring idx slots][sync wb 0x104][2 x 8-byte
   slots][sync blk 0x1000][rupt wb 0x20]`. All offsets are relative to pool / pool_ba.
4. copy_to_user 112 → 0 / -EACCES. No hardware access, no state change.

## 3. nr 3 — RUPT_MASK (0xa304, inlined inno_rupt_mask, ipd_main.c ~294-322)
Input only (44 bytes); nothing is copied back.
1. copy-in, instance/version checks.
2. `v = arg.vector (u16 @+8)`; `v > 6` → -EINVAL. `idev->rupts[v].vector (u16 @+0x8600+96*v) == 0` →
   `IPD-ERR: vector has no irq: %x %x` and -EINVAL.
3. Compute the MSI-X routing number `msix_num` (w27):
   ```
   a574 mov w27,w3                   ; = v
   a578 ldr w0,[x24,#1532]           ; idev->num_vectors (+0x85fc)
   a57c cmp w3,w0 ; b.lt a58c ; a584 sub w0,w0,#1 ; uxth w27,w0   ; if v >= num_vectors: msix_num = num_vectors-1
   a590 ldr w1,[idev+0x9c14]         ; idev->flags
   a594 tbz w1,#0,a59c ; a598 ldrh w27,[idev+0x9c10]              ; if flags & 1: msix_num = (u16)msi_wa_vector
   ```
4. For i = 0..7 (word index), with `m = &idev->rupts[v].mask[i]` (+0x8604+96*v+4*i):
   * old = *m. If old != 0:
     `W BAR0+0x182a0+4*i (INTR_INMC + 4i) = old` then `W BAR0+0x18240+4*i (INTR_INCR + 4i) = old`
     (mask-clear = disable the previously owned sources, then clear their cause bits).
     (a5f8..a608; debug strings "Writing INTR_INMC(0x%x) + i*4" / "Writing INTR_INCR(0x%x) + i*4".)
   * `*m = new = arg.mask[i]` (arg+0x0c+4*i) (a60c..a618).
   * If new != 0: for each bit b (0..31) set in new:
     `W BAR0+0x18400 + 4*(32*i + b) (INTR_IMMR + 4*(32i+b)) = msix_num`
     (`a620 add w20,w28,#0xc20 ; a634 lsl w20,w20,#5` with w28 = 4*i → 0x18400+0x80*i, +4 per bit;
     string "Setting INTR_IMMR + %d msix_num: %d"). Then
     `W BAR0+0x18280+4*i (INTR_INMS + 4i) = new` (mask-set = enable; string "Writing INTR_INMS + %d").
5. `idev->rupts[v].flag = 1` (a6d0) and `__wake_up(&idev->rupts[v].wait_q, TASK_INTERRUPTIBLE(1), 1, NULL)`
   i.e. wake_up_interruptible → any waiter in RUPT_WAIT on that vector returns immediately.
6. return 0.
Notes: the 8-word mask is 256 interrupt-source bits; source number s = 32*i + b maps to register
INTR_IMMR[s] (one 32-bit routing register per source, 0x18400 + 4*s) and to bit b of
INTR_INMS/INMC/INCR word i. **`idev->immr[40]` (+0x8990) is NOT touched by this ioctl**, and no
spinlock is taken here. Registers written: INTR_INMC[i], INTR_INCR[i], INTR_IMMR[s], INTR_INMS[i].

## 4. nr 4 — RUPT_WAIT (0x9b90, inlined inno_rupt_wait, ipd_main.c ~355-386)
1. copy-in 64, instance/version checks.
2. `v = arg.vector (u32 @+0x18)`; `v > 6` → -EINVAL; `rupts[v].vector == 0` → -EINVAL.
3. Re-arm: `spin_lock_irqsave(&idev->rupt_lock /*+0x8a40*/)`; for i=0..7: if `rupts[v].mask[i] != 0`:
   `W BAR0+0x18280+4*i (INTR_INMS + 4i) = rupts[v].mask[i]`; unlock.
   ```
   9c34 mov x2,#0x8280 ; movk x2,#1,lsl#16      ; 0x18280
   9c44 mov x4,#-64636 ; 9c48 add x3,x19(=96*v),x2 ; add x3,x3,x4   ; = 0x8604+96v+4i
   9c50 ldr w3,[x23,x3] ; cbz ; 9c58 ldr x5,[idev+0x8030] ; 9c60 str w3,[x5,x2]
   ```
   If **all 8 mask words are zero** (no RUPT_MASK done for this vector) → return **-1 (-EPERM)**
   (9c80 → 960c), nothing copied back.
4. Wait on `rupts[v].wait_q` for `rupts[v].flag != 0` (`flag` @+0x8644+96*v is set by the ISR —
   not in this function — and by RUPT_MASK):
   * `arg.timeout (u64 @+8) == 0` → `wait_event_interruptible(wait_q, flag != 0)` (a6dc..a73c, uses
     `schedule`). Its return value is **discarded**; `remaining_time` is left as the caller passed it.
   * else `ret = wait_event_interruptible_timeout(wait_q, flag != 0, msecs_to_jiffies((u32)timeout))`
     (9c8c, a440..a4d0, `__msecs_to_jiffies` + `schedule_timeout`, state TASK_INTERRUPTIBLE);
     `arg.remaining_time (u64 @+0x10) = (long)ret` (9cbc `str x27,[x29,#304]`).
     So: **timeout is in milliseconds (only the low 32 bits are used); remaining_time is in jiffies**:
     0 = timed out with flag still clear; >= 1 = flag set (remaining jiffies, min 1);
     on a signal it is **-ERESTARTSYS (0xfffffffffffffe00)** (b240 `mov x27,x0`).
5. Consume (always executed — after success, timeout and signal alike):
   `spin_lock_irqsave(rupt_lock)`; `rupts[v].flag = 0`; `memcpy(arg.rupts /*+0x1c*/, rupts[v].pend /*+0x8624+96v*/, 32)`;
   unlock (9cc0..9d00). **`pend[]` is only copied, not cleared here** (whoever sets it — the ISR — owns it).
6. copy_to_user 64 → **returns 0 even on timeout or signal** (-EACCES only on copy failure). Userspace
   must look at remaining_time / rupts[] to tell the cases apart; the ioctl is never restarted and
   never returns -EINTR/-ERESTARTSYS.
The interrupt sources are NOT re-masked here; the expected protocol (INFERRED) is: ISR writes
INTR_INMC for the fired sources, latches them in pend[], sets flag, wakes; the next RUPT_WAIT
re-enables through INTR_INMS (step 3).

## 5. nr 5 — "Read" (0x9b30) and nr 6 — "write" (0x9ad0)
No header/version/instance check. The 8-byte argument is used as `{u32 offset; u32 value}`
(it is sized like inno_ioctl_hdr_t but not interpreted as one).
* **nr 5**: copy_from_user 8; `value = *(u32 *)(X + offset)`; copy_to_user 8; return 0.
  ```
  a75c add x23,x23,#0x8,lsl #12 ; a760 ldr w0,[x29,#288]
  a768 ldr x2,[x23,#2608]        ; idev+0x8a30 = idev->pool   (!!)
  a76c ldr w0,[x2,x0] ; a770 str w0,[x29,#292]
  ```
  VERIFIED: the base is `idev->pool` (+0x8a30, the DMA pool virtual address), **not** bar0.
* **nr 6**: copy_from_user 8; `*(u32 *)(idev->bar0 + offset) = value`; return 0; no copy back.
  ```
  a7dc add x23,x23,#0x8,lsl #12 ; a7e0 ldr w0,[x29,#288] ; a7e4 ldr w2,[x29,#292]
  a7ec ldr x1,[x23,#48]          ; idev+0x8030 = idev->bar0
  a7f0 str w2,[x1,x0]
  ```
  So read is from the pool and write is to BAR0 — asymmetric, but that is what the binary does
  (plain `ldr`/`str`, no readl/writel wrappers, no bounds check on offset in either).
* Errors: copy-in failure → memset of the uncopied tail, `IPD-ERR: Read/write Unable to get from user`,
  -EACCES. nr 5 copy-out failure → `Read Unable to put to user`, -EACCES.

## 6. nr 7 — ALLOC_RING (0x9a30, inlined inno_alloc_ring, ipd_main.c ~789-1082)
inno_ring_flags_t: INIT=1, ENABLE=2, NETDEV=4, TX=8. Arg fields: ring@8, wb@0x10, count@0x18(u16),
vector@0x1a(u16), flags@0x1c, num@0x20(u8), idx_offset@0x24, cidx@0x28(u16), pidx@0x2a(u16).

**Address model: the user does not supply any address.** The kernel allocates descriptor ring,
writeback ring (and tx_cksum) with dma_alloc_coherent and returns their **bus addresses** in
`arg.ring` / `arg.wb`; `arg.idx_offset` is an **offset into the DMA pool** (pool/pool_ba) of the
4-byte index-writeback word for this ring. Input values of ring/wb/idx_offset/cidx/pidx are ignored.
(How userspace maps those bus addresses is the mmap handler's business — not in this function.)

1. copy-in 48, instance/version checks.
2. `TX = arg.flags & 8`. TX: `arg.num > 4` → `IPD-ERR: TX Ring number too large`, -EINVAL;
   ring = &idev->tx_ring[num] (+0x8048+112*num); desc bytes = `count*16` (a8f0 `lsl w25,w3,#4`).
   RX: `arg.num > 4` → `RX Ring number too large`, -EINVAL; ring = &idev->rx_ring[num]
   (+0x8358+112*num); desc bytes = `count*8`. (Both limits are 4 = `cmp w0,#4; b.ls`, although the
   arrays are [7]/[6].)
3. If `ring->flags & INNO_RING_INIT` already: `IPD-ERR: Ring already intialized`, then fall through to
   step 12 (fill outputs from the existing ring, copy_to_user, return 0). Not an error.
4. `memset(ring, 0, 112)`; `ring->num = arg.num`.
5. Flag/idx setup (A = ALIGN(4, idev->cache_align) = (ca+3) & -ca):
   * NETDEV (arg.flags & 4): `ring->flags |= 4`; local `vec = 0`.
     * NETDEV+TX: `idev->enet_tx_ring_num (+0x8b1c) = num`;
       `idev->napi_mask[0] (+0x8af8) |= 1 << inno_txq_int[num]`.
     * NETDEV+RX: `idev->napi_mask[0] |= 1 << inno_rxq_int[num]`. (enet_rx_ring_num is not written here.)
   * not NETDEV: local `vec = arg.vector`.
   * TX: `ring->flags |= 8`; `ring->idx_offset (+0x38) = A * num` ("TX_IDX_OFFSET").
   * RX: `ring->idx_offset = A * (num + 7)` ("RX_IDX_OFFSET").
   * `ring->count (+0x34) = arg.count`.
   Tables (.data): `inno_txq_int[7] = {4,5,6,7,8,9,10}`, `inno_rxq_int[8] = {11,12,13,14,15,16,17,18}`
   (interrupt source numbers, i.e. bit positions in INTR word 0 / index into INTR_IMMR).
6. Allocations (dev = &idev->pdev->dev; all dma_alloc_coherent with gfp 0x24000c0 = GFP_KERNEL of 4.9):
   * `ring->{tx,rx}_desc (+0x10) = dma_alloc_coherent(dev, descbytes, &ring->desc_ba (+0x40))`;
     NULL → `Descriptor ring alloc Failed`, **-ENOMEM (-12)**.
   * TX only: `ring->tx_cksum (+0x50) = dma_alloc_coherent(dev, count*64, &ring->tx_cksum_ba (+0x58))`;
     NULL → `TX checksum descriptor alloc failed`, free desc, -ENOMEM.
   * `ring->{tx,rx}_wb (+0x18) = dma_alloc_coherent(dev, descbytes /*same size as desc ring*/, &ring->wb_ba (+0x48))`;
     NULL → `Writeback ring alloc Failed`, free desc (+ tx_cksum if TX), -ENOMEM.
     `memset(wb, 0, descbytes)`. (The descriptor ring itself is not zeroed here.)
   * NETDEV only: `ring->desc_info (+0x60) = kmalloc(count*32, GFP_KERNEL)` (sizeof inno_ring_desc_info_t=32);
     NULL → `RING desc info alloc Failed`, free wb, desc (+tx_cksum), -ENOMEM. Then
     `ring->cidx_addr/pidx_addr (+0x20) = idev->pool + ring->idx_offset` (ab64..ab74) and memset(desc_info,0).
     **For non-NETDEV rings +0x20 stays NULL and desc_info stays NULL.**
   * Always: `ring->pages (+0x68) = kmalloc(count*24, GFP_KERNEL)` (sizeof inno_dma_alloc_t=24), zeroed;
     NULL → -ENOMEM **without freeing anything** (be84 → af50).
   * `ring->idev = idev`.
7. Queue quiesce: TX → `inno_disable_queue_tx(idev, num)`; RX → `inno_disable_queue_rx(idev, num)`.
   If the result is negative: `IPD-ERR: inno_disable_queue_{tx,rx} failed for {Tx,Rx} ring number %d`,
   remember `err = -62 (-ETIME)` and **continue** with the whole programming sequence below.
8. TX register programming (q = num; offsets from tables at .rodata+0x7a8.., stride 0x48 per TX queue):
   ```
   W BAR0+0x413c+0x48*q (TXQ_q_DESC_RING)               = ring->count
   W BAR0+0x4128+0x48*q (TXQ_q_BASE_HSN_HI)             = desc_ba >> 32
   W BAR0+0x4124+0x48*q (TXQ_q_BASE_HSN_LO)             = (u32)desc_ba
   W BAR0+0x4130+0x48*q (TXQ_q_BASE_HSN_HI_WB)          = wb_ba >> 32
   W BAR0+0x412c+0x48*q (TXQ_q_BASE_HSN_LO_WB)          = (u32)wb_ba
   ring->pidx (+0x30) = 0 ; *(u32 *)(idev->pool + idx_offset) = 0 ; ring->cidx (+0x2c) = 0
   W BAR0+0x4138+0x48*q (TXQ_q_CIDX_WB_HSN_HI)          = (pool_ba + idx_offset) >> 32
   W BAR0+0x4134+0x48*q (TXQ_q_CIDX_WB_HSN_LO)          = (u32)(pool_ba + idx_offset)
   W BAR0+0x4144+0x48*q (TXQ_q_CIDX_UPDATE)             = 0
   W BAR0+0x4148+0x48*q (TXQ_q_CIDX_UPDATE_CLIFF)       = 0
   W BAR0+0x414c+0x48*q (TXQ_q_CIDX_UPDATE_PRESCALER)   = 0
   W BAR0+0x4150+0x48*q (TXQ_q_CIDX_UPDATE_PRECLIFF)    = 0
   W BAR0+0x4154+0x48*q (TXQ_q_CIDX_UPDATE_TMR_CTL)     = 2
   if NETDEV: inno_enable_queue_tx(idev, num)      (bdb4; return value ignored)
   ```
9. RX register programming (stride 0x44 per RX queue):
   ```
   W BAR0+0x468c+0x44*q (RXQ_q_CIDX_UPDATE)             = 0
   W BAR0+0x4690+0x44*q (RXQ_q_CIDX_UPDATE_CLIFF)       = 0
   W BAR0+0x4694+0x44*q (RXQ_q_CIDX_UPDATE_PRESCALER)   = 0
   W BAR0+0x4698+0x44*q (RXQ_q_CIDX_UPDATE_PRECLIFF)    = 0
   W BAR0+0x469c+0x44*q (RXQ_q_CIDX_UPDATE_TMR_CTL)     = 2
   W BAR0+0x46a0+0x44*q (RXQ_q)                         = 0x00030001
   W BAR0+0x4670+0x44*q (RXQ_q_BASE_HSN_HI)             = desc_ba >> 32
   W BAR0+0x466c+0x44*q (RXQ_q_BASE_HSN_LO)             = (u32)desc_ba
   W BAR0+0x4678+0x44*q (RXQ_q_BASE_HSN_HI_WB)          = wb_ba >> 32
   W BAR0+0x4674+0x44*q (RXQ_q_BASE_HSN_LO_WB)          = (u32)wb_ba
   if NETDEV && count != 0: for i in 0..count-1:            (b5f4..b6a0)
        pages[i].page = alloc_pages(gfp=0x2, order 0)        ; see note on gfp in section 11
        if NULL: printk "IPD_CRIT: Unable to allocate RX page memory ring: %d, index: %d" and BREAK
                 (no error returned, remaining descriptors left unfilled)
        pages[i].dma_addr = dma_map_page(dev, page, 0, 0x1000, DMA_FROM_DEVICE(2))   (no mapping-error check)
        pages[i].vmaddr   = page_address(page)
        rx_desc[i].hsn_upper = dma >> 32 ; rx_desc[i].hsn_lower = (u32)dma          (dmb before/after)
   ring->pidx (+0x30) = 0x8000 ; *(u32 *)(idev->pool + idx_offset) = 0 ; ring->cidx (+0x2c) = 0     (b710..b728)
   W BAR0+0x4680+0x44*q (RXQ_q_CIDX_WB_HSN_HI)          = (pool_ba + idx_offset) >> 32
   W BAR0+0x467c+0x44*q (RXQ_q_CIDX_WB_HSN_LO)          = (u32)(pool_ba + idx_offset)
   W BAR0+0x4684+0x44*q (RXQ_q_DESC_RING)               = ring->count
   if NETDEV: inno_enable_queue_rx(idev, num)      (b7d4; return value ignored)
   ```
   (Each table lookup is guarded `num > 5 → offset 0xffffffff`; unreachable because num <= 4.)
10. Common tail (ae20..ae58), qint = inno_txq_int[num] or inno_rxq_int[num]:
    ```
    W BAR0+0x18400 + 4*qint (INTR_IMMR[qint]) = vec        ; arg.vector for user rings, 0 for NETDEV rings
    W BAR0+0x18280 (INTR_INMS word 0)         = idev->napi_mask[0]
    ring->flags |= INNO_RING_INIT (1)
    ```
    `ae24 mov w3,#0x6100 ; add w0,w0(qint),w3 ; lsl w0,w0,#2` → 0x18400+4*qint.
11. (debug) `Ring allocated %s%d ring: %p cidx: %x pidx: %x wb: %p info: %p`.
12. Outputs: `arg.ring = ring->desc_ba`, `arg.wb = ring->wb_ba`, `arg.idx_offset = ring->idx_offset`,
    `arg.cidx = (u16)ring->cidx`, `arg.pidx = (u16)ring->pidx` (so RX returns pidx 0x8000, TX 0).
    count/vector/flags/num echo the input.
13. If err (the -62 from step 7) != 0 → return -62 with **no copy_to_user** (ring is nevertheless left
    INIT). Else copy_to_user 48 → 0 / -EACCES.

## 7. nr 8 — FREE_RING (0x9984)
copy-in 48; checks; `inno_free_ring(idev, arg.flags & 8 /*tx*/, arg.num)` (w1 = flags&8, w2 = num);
non-zero → return it; else copy_to_user 48 (unmodified echo) → 0.

## 8. nr 23 — ENABLE queue (0x9450) / nr 24 — DISABLE queue (0x9178)
copy-in 48; checks; `num = arg.num`, then
* nr 23: `arg.flags & 8 ? inno_enable_queue_tx(idev, num) : inno_enable_queue_rx(idev, num)`
* nr 24: `arg.flags & 8 ? inno_disable_queue_tx(idev, num) : inno_disable_queue_rx(idev, num)`
non-zero → return it (sign-extended); else copy_to_user 48 (echo) → 0. No range check on num in the
dispatcher.

## 9. nr 9/10 HRR, nr 11/12 LEARN, nr 13 TX_SEND
* nr 9: copy-in 32; checks; `dmb ish` (smp_mb); `inno_alloc_hrr(idev, &arg)`; non-zero → return it;
  `dmb ish`; copy_to_user 32 → 0.
* nr 10: copy-in 32; checks; `inno_free_hrr(idev)`; non-zero → return; copy_to_user 32 (echo) → 0.
* nr 11: copy-in 40; checks; `inno_alloc_learn(idev, &arg)`; non-zero → return; copy_to_user 40 → 0.
* nr 12: copy-in 40; checks; `inno_free_learn(idev)`; non-zero → return; copy_to_user 40 (echo) → 0.
* nr 13: copy-in 64; checks; `inno_tx_send(idev, &arg)`; non-zero → return; copy_to_user 64 → 0.

## 10. nr 14 — UNPIN_PAGES (0x9748, inlined inno_unpin_page)
Arg: hrr@8(u8, **never read**), ring_num@9(u8), idx@0xa(u16). **No bounds checks at all** on ring_num
or idx, no INIT check, no NULL check of the ring arrays.
1. copy-in 12; checks.
2. `ring = &idev->tx_ring[ring_num]`; `d = &ring->tx_desc[idx]`;
   if bit 1 of byte 12 of the descriptor is set (`97c8 ldrb w2,[x2,#12]; tbnz w2,#1` — per DWARF
   that is the `eop` bit; dd=bit0, eop=bit1, sop=bit2 of the LE word at +0xc) → skip to step 4.
3. `p = &ring->pages[idx]`; if `p->page == NULL` → skip. Else
   `dma_unmap_page(dev, p->dma_addr, 0x1000, DMA_TO_DEVICE(1))`; then
   * `p->vmaddr != NULL` (kernel page from RING_PAGE_ALLOC) → `__free_pages(p->page, 0)`
   * `p->vmaddr == NULL` (user page pinned by the zero-copy send path) → `put_page(p->page)` (aec0..)
   then `p->vmaddr = NULL; p->page = NULL` (dma_addr left stale).
4. copy_to_user 12 (echo) → 0.

## 11. nr 17 — RING_PAGE_ALLOC (0x9614) / nr 18 — RING_PAGE_FREE (0x9590)
Arg: buf_ba@8(u64), ring_type@0x10 (TX=1,RX=2,HRR=3,LEARN=4), ring_num@0x14(u8), idx@0x18(u32).
All failures inside the inlined helpers return **-1 (-EPERM)** (`mov x19,#-1`) with an IPD-ERR print.

### ALLOC (inlined inno_ring_page_alloc, ~1931-2026)
copy-in 32; checks; then by ring_type:
* **TX(1)**: ring_num > 6 → `Invalid TX ring %u`; `!(tx_ring[n].flags & INIT)` → `TX Ring %u uninitialized`;
  `idx >= tx_ring[n].count` → `Invalid TX ring %u index %u`; `tx_ring[n].pages == NULL` →
  `Pages array not allocated for TX ring %u`. slot = &pages[idx]; dir = DMA_TO_DEVICE(1).
* **RX(2)**: ring_num > 5 → `Invalid RX ring %u`; not INIT → `RX Ring %u uninitialized`;
  `idx >= count` → `Invalid RX ring %u index %u`; pages NULL → `Pages array not allocated for RX ring %u`.
  slot = &pages[idx]; dir = DMA_FROM_DEVICE(2).
* **HRR(3)**: `idev->hrr.pages (+0x9b90) == NULL` → `Async FIFO buffer array unallocated`;
  `idx >= idev->hrr.descriptors (+0x9b80)` → `Invalid Async FIFO buffer array index %u`.
  slot = &hrr.pages[idx]; dir = DMA_FROM_DEVICE(2). ring_num ignored.
* anything else (incl. LEARN=4) → -1.
Then:
```
slot->page     = alloc_pages(gfp = 0x2, order = 0)      ; __alloc_pages_nodemask(w0=2, w1=0, zonelist, NULL)
                 NULL -> "Unable to allocate page buffer buffer for index %u", -1
slot->dma_addr = dma_map_page(dev, page, 0, 0x1000, dir)   ; no dma_mapping_error check
slot->vmaddr   = page_address(page)
arg.buf_ba     = slot->dma_addr
RX only: rx_ring[n].rx_desc[idx].hsn_upper = dma >> 32 ; .hsn_lower = (u32)dma   (dmb around; bb48..bb7c)
```
copy_to_user 32 → 0 / -EACCES. An already-populated slot is overwritten without freeing.
gfp note: the literal mask is 0x2 (= `__GFP_HIGHMEM` in 4.9, i.e. no reclaim/IO/FS bits → effectively a
non-sleeping allocation). INFERRED: the source probably passed a DMA-direction constant by mistake; a
replacement can use GFP_KERNEL (or GFP_ATOMIC) with order 0. For TX and HRR the kernel does **not**
write the address into any descriptor — userspace gets buf_ba and does that itself.

### FREE (inlined inno_ring_page_free, ~2054-2102)
copy-in 32; checks; same type switch and the same validations/messages as ALLOC, **except** the
TX/RX `pages == NULL` check is absent (HRR keeps its NULL check). Directions: TX 1, RX 2, HRR 2. Then
`dma_unmap_page(dev, slot->dma_addr, 0x1000, dir)`; `__free_pages(slot->page, 0)` (no NULL check on
slot->page); `slot->dma_addr = slot->page = slot->vmaddr = 0` (b224..b238). **return 0 with no
copy_to_user.**

## 12. nr 19/20/21 — PIC status chain (inno_pic_st_t @ idev+0x9bf8: wb_vmaddr, count@+8, wb_ba@+0x10)
Arg: wb_ba@8 (u64, out), count@0x10 (u32, in).

### nr 19 PIC_ST_ALLOC (inlined inno_pic_st_alloc, ~1359-1375)
copy-in 24; checks.
* If `pic_st.wb_vmaddr == NULL`: `pic_st.count = arg.count`;
  `pic_st.wb_vmaddr = dma_alloc_coherent(dev, (u64)count * 4, &pic_st.wb_ba, GFP_KERNEL)`;
  NULL → `Writeback PIC status chain alloc Failed`, **-ENOMEM**.
  (If already allocated the existing buffer is reused and arg.count is ignored.)
* Then in both cases (9400..9448 / c014..c054):
  ```
  W BAR0+0x18f4c (INTR_PSC_AHI) = pic_st.wb_ba >> 32
  W BAR0+0x18f48 (INTR_PSC_ALO) = (u32)pic_st.wb_ba
  W BAR0+0x18f50 (INTR_PSC_TMR) = 1000000 (0xf4240)
  arg.wb_ba = pic_st.wb_ba
  ```
* copy_to_user 24 → 0.

### nr 20 PIC_ST_FREE (inlined inno_pic_st_free)
copy-in 24; checks; `pic_st.wb_vmaddr == NULL` → **-EINVAL**. Else
`W INTR_PSC_TMR (0x18f50) = 0; W INTR_PSC_AHI (0x18f4c) = 0; W INTR_PSC_ALO (0x18f48) = 0;`
`dma_free_coherent(dev, pic_st.count*4, wb_vmaddr, wb_ba)`; `pic_st.wb_vmaddr = NULL` (count/wb_ba kept);
copy_to_user 24 (echo) → 0.

### nr 21 PIC_ST_DUMP (inlined inno_pic_st_dump / inno_port_st_dump)
copy-in 24; checks; `wb_vmaddr == NULL` → -EINVAL. Pure diagnostics: if ipd_loglevel > 3 prints a
header and decodes **256 fixed 32-bit words** (0x400 bytes, regardless of pic_st.count) of the wb
buffer as bitfields (bits 0-2 ib, 3-5 pic, 6-8 lane, 9-12 speed, 13-14 fec, 15 en, 16 signal,
17 an_master, 18 an_training, 19 link_up, 20 rx_link_active, 21 tx_link_active, 22 local_flt,
23 remote_flt, 24 err, 25 parity — column order per the header string). No state change, no register
access. copy_to_user 24 (echo) → 0.

## 13. nr 15/16 NETDEV create/delete, nr 25/26 NETDEV info get/set
* nr 15: copy-in 48; checks; `inno_netdev_create(idev, &arg)`; non-zero → return it; else
  `idev->inno_netdev.num_interfaces (+0x9e74, u16) += 1`; return 0. No copy back.
* nr 16: copy-in 48; checks; `inno_netdev_delete(idev, arg.sysport /*u16 @+8*/)`; non-zero → return;
  `num_interfaces -= 1`; return 0. No copy back.
* nr 26 (SET): copy-in 76; checks; `inno_netdev_set(idev, arg)` — the 76-byte struct is passed **by
  value** (copied to x29+0xa0, pointer in x1 per AAPCS64); returns the callee's value (0 or error).
  No copy back.
* nr 25 (GET): copy-in 76; checks; `inno_netdev_get(idev, &arg)`; non-zero → return; copy_to_user 76 → 0.

## 14. Error/return summary
| condition | return |
|---|---|
| nr 0 / 22 / > 26 | -EINVAL (-22) |
| access_ok or copy_{from,to}_user failure | **-EACCES (-13)** |
| hdr.version != 1, hdr.instance > inno_num_devices | -EINVAL |
| RUPT_MASK/RUPT_WAIT vector > 6 or rupts[v].vector == 0; QUERY lookup miss; ALLOC_RING num > 4; PIC free/dump with nothing allocated | -EINVAL |
| RUPT_WAIT with an all-zero stored mask; unknown ring_type and every RING_PAGE_ALLOC/FREE validation or page-alloc failure | -1 (-EPERM) |
| ALLOC_RING / PIC_ST_ALLOC allocation failure | -ENOMEM (-12) |
| ALLOC_RING: inno_disable_queue_* < 0 | -62 (-ETIME), ring still programmed and marked INIT |
| non-zero from inno_alloc_hrr / free_hrr / alloc_learn / free_learn / free_ring / tx_send / netdev_* / enable/disable_queue | that value, verbatim |
| RUPT_WAIT timeout or signal | **0** (see remaining_time) |

## 15. Could not determine / caveats
* The real ioctl macro names and the encoded dir/size bits (only the low byte is decoded; the
  0x80084901 constant is the single hint). Names in the table are proposals.
* Who sets `rupts[v].flag` / `pend[]` and writes INTR_INMC on interrupt (ISR — outside this function),
  and whether `immr[40]` is used elsewhere (it is not referenced in inno_ioctl).
* HZ of the target kernel (unit of remaining_time is jiffies).
* Why nr 5 reads from `pool` while nr 6 writes to `bar0` (verified in the binary; intent unknown).
* Semantics of RXQ_q = 0x30001 and CIDX_UPDATE_TMR_CTL = 2 bitfields (see the register structs in
  types.txt; not decoded here).
* Bodies of all non-inlined callees (by brief).
