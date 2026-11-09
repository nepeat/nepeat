# Cold boot and flash acquisition — 2026-11-08

## Results and current state

The user authorized power cycling for a complete boot log, followed by breaking
into the bootloader to dump firmware. Both tasks succeeded. At completion of
acquisition the device was left at `CFE>`. It was subsequently booted into a
[Linux root shell](linux-shell.md); attach with `tmux attach -t serial-bcm956845`.
Console: 9600 8N1, no flow control. No flash programming/erase, partition-table
save, persistent environment update, memory test or alternate-OS boot occurred
during the acquisition phase.

- [Complete cold-boot log](boot-20261108-184531.txt), from CFE through `BCM.0>`.
- [CFE acquisition transcript](cfe-session-20261108.txt).
- Continuous live raw log: [serial-20261108-183606.txt](serial-20261108-183606.txt).
- [Acquisition manifest](dumps/acquisition.json), [repeat-read checks](dumps/verification.json),
  [SHA256SUMS](dumps/SHA256SUMS), [transfer records](dumps/transfers.jsonl).
- [Extracted file manifest](firmware/manifest.json).

Raw `.bin` acquisitions and large extracted firmware are local and ignored by
git. Keep/copy the actual binaries alongside their manifests when archiving;
committing these notes alone does not preserve the binary backups.

## Cold boot

First cycle used Home Assistant `switch.server_room_lab_plug`: verified off,
waited three seconds, verified on. No serial input sent during initialization.
The serial pane reached `BCM.0>` before tio's log flushed the trailing prompt;
a single Enter completed that line in the capture. Time spent waiting for a
log flush must not be interpreted as measured boot duration.

Second cycle used the same outlet sequence. After observing a fresh CFE banner,
sent Ctrl-C repeatedly until `Automatic startup canceled via Ctrl-C` and `CFE>`.
The initial interrupt-only capture is partial because of buffering; the
continuous log and consolidated CFE transcript contain the complete dialogue.

Boot observations:

- `CFE-NTSW-5.1.2 for BCM98548XMC (32bit,SP,BE,PPC)`, built 2009-12-01.
- Board ID `0x51`; CPU type `0x80210022`; core 997 MHz / CPU 998 MHz;
  CCB 399 MHz; RAM `0x20000000` (512 MiB).
- PCI switch device `0xb845`, revision `0x04`; SDK identifies **BCM56845_A3**.
- PHY initialization messages for ports 1–64 identify 84740 ROM code `0xd00c`.
  These software port messages do not establish the physical connector count
  or active 40G configuration.
- VxWorks network boot parsing and WDB agent configuration report failures;
  SDK still initializes and reaches its prompt. Errors are preserved in the log.

## Flash mappings and acquired images

CFE `show devices` reports:

| Name | Base | Size | Partitions |
| --- | --- | --- | --- |
| `flash2` | `0xf8000000` | `0x08000000` (128 MiB) | `flash0.os`: first 127 MiB; `flash0.boot`: last 1 MiB |
| `flash3` | `0xf0000000` | `0x08000000` (128 MiB) | `flash1.os`: first 127 MiB; `flash1.boot`: last 1 MiB |

`show flash flash2` confirms size `08000000`, type Flash. These are mapped
regions; physical chip count/markings still need board inspection.
`ptable show` returned no named entries even though the built-in device aliases
above exist. Do not create or save a partition table to investigate this.

Each mapping was acquired in sixteen contiguous 8 MiB chunks using CFE `save`.
Concatenation in address order produced:

| Local file | Bytes | SHA-256 |
| --- | ---: | --- |
| `dumps/flash2-full.bin` | 134,217,728 | `8f528916444600f5a226cd7ab7a22306ade452fb7064c9f99eb4425fc7b23841` |
| `dumps/flash3-full.bin` | 134,217,728 | `8748d07613daea85b21de4f02da19221f5dde31ee1225240dd64477d855f0d23` |

Active FAT16 filesystem is at the start of `flash2`. The `flash3` OS region
is erased (`0xff`); its final 1 MiB contains the same boot-region bytes as
`flash2`. Identical boot copies could reflect duplication or board mapping;
physical independence has not been established. Extracted `flash2-boot.bin`
and `flash3-boot.bin` share SHA-256:
`21dda0768b3f3c168599da7667b91e6056e0f70888e44500768e0b2bdcb92633`.

## TFTP failure and workaround

CFE `save` advertises a 1428-byte TFTP block size. Packet capture established:

1. CFE WRQ with `blksize=1428`; server replies OACK.
2. CFE sends initial **DATA block 0**, rather than block 1.
3. Normal atftpd ACKs block 0 without writing it, then writes blocks 1 onward.
4. CFE reports successful full length even though the server lost 1428 bytes.

Both initial 4096-byte probes produced only 2668 bytes, beginning at flash
offset `0x594` (1428). This occurred on both camera and lab VLANs; it was not
a VLAN transport issue. A serial `d -b` at that offset matched the truncated
file's beginning. Failed probes are retained as `dumps/tftp-truncated-*.bin`;
they are **not valid firmware backups**. Packet evidence: local ignored
`tftp-probe-lab.pcap`.

The host already ran atftpd on UDP/69 with root `/srv/tftpboot`. An attempted
second atftpd could not bind that port. Existing host service/configuration was
left intact. A dedicated staging directory enabled the early atftpd probes;
it was removed after retaining their results locally.

[cfe_tftp_receiver.py](cfe_tftp_receiver.py) accepts either initial DATA 0 or 1,
validates the sending IP, handles repeated blocks, checks negotiated block
sizes, hashes data and publishes complete files after fsync. It was run as the
normal user on host `10.60.2.208:1069`, writing directly to this device's
`dumps/` directory. Two temporary host iptables rules redirected only switch
`10.60.2.203` → host `10.60.2.208` UDP/69 to 1069 and allowed that scoped
input. Both rules and the receiver were removed after acquisition. No host
firewall/service configuration was persisted.

Network: initially DHCP `10.60.20.203/24` (camera VLAN). User moved the switch
port to lab VLAN; `ifconfig eth0 -auto` then obtained `10.60.2.203/24`, gateway
`10.60.2.1`. CFE successfully pinged host `10.60.2.208`. This configuration is
runtime only; no CFE environment save was used.

## Acquisition verification

- Corrected 4 KiB sample had exactly 4096 bytes; first 64 bytes matched serial
  dump at `0xf8000000` byte-for-byte.
- All 32 chunks were exactly 8 MiB; hashes recorded before concatenation.
- Both combined images are exactly 128 MiB.
- Five independent repeat reads matched corresponding full-image slices:
  active-bank start (4 KiB), Linux-image area at `0xf9000000` (64 KiB), active
  boot region at `0xfff00000` (64 KiB), second-bank start (4 KiB), and second
  boot region at `0xf7f00000` (64 KiB).
- All six FAT16 files extracted with lengths matching the live VxWorks listing.
- `bcm.gto.gz` passed gzip integrity validation; decompressed size 19,434,556,
  ELF magic `7f454c46` (32-bit, big-endian).
- Linux uImage header CRC `0x86c787c9` and data CRC `0x0211d9e1` both verified.
  Name `gto-2_6`, image timestamp 2011-01-13 22:44:08 UTC, Linux/PowerPC,
  multi-file/gzip. Two payloads: 1,681,403 and 9,720,364 bytes; both gzip CRCs
  pass. Expanded sizes: 3,563,776 and 30,185,472 bytes respectively.
- First payload contains version string:
  `Linux version 2.6.21.7-hrt1-WR2.0bl_standard`, GCC 4.1.2 Wind River,
  build `#3 PREEMPT Wed Jun 17 09:08:06 PDT 2009`.
  Second payload's purpose/root filesystem format is not yet analyzed.

These are checks of one full acquisition plus independent selected reads,
not two independently acquired full-image copies. EEPROM and PHY SPI ROMs
are not included in the two CPU flash mapping dumps.

## Reproduction helpers

Inside the nix devshell:

```sh
# At a fresh CFE prompt; waits for each command's prompt before returning.
python3 devices/broadcom-bcm956845k-02/cfe_query.py 'show devices'
python3 devices/broadcom-bcm956845k-02/cfe_query.py 'ifconfig eth0 -auto'

# Receiver: confirm current addresses and arrange a scoped UDP/69 redirect first.
python3 devices/broadcom-bcm956845k-02/cfe_tftp_receiver.py \
  --bind 10.60.2.208 --source 10.60.2.203 --port 1069 \
  --root devices/broadcom-bcm956845k-02/dumps

# Example CFE command (one 8 MiB chunk; use a fresh destination name):
# save 10.60.2.208:flash2-f8000000.bin 0xf8000000 0x00800000

# Offline extraction into a NEW destination; existing files aren't overwritten.
python3 devices/broadcom-bcm956845k-02/extract_fat16.py \
  devices/broadcom-bcm956845k-02/dumps/flash2-full.bin /tmp/bcm-fat16-extraction
```

One early command batch overran/lost console input (`printenv` + `help save`
became `prihelp save`). Reissued individual queries successfully; the helper
now waits for a new exact command echo and completed prompt. `help dump`
was invalid; correct memory-read command is `d`. Bare `show flash` printed
usage; `show flash flash2` worked. These failures did not write device state.
