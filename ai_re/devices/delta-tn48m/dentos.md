# dentOS install found on the SSD (as received)

Source: `dumps/sda.img.zst` (sha256 of the raw image
`a7d658a888f193c6b96ed5181e9d3357c9a65ec3e894fbabab54337ec40721dd`,
matching on-device `sha256sum /dev/sda`). Read offline with debugfs. Extracts
are in `dumps/extracted/` (gitignored; see "sensitive material").

## layout (DOS MBR, label-id 0x0006a9c1)

| part | start (sector) | size | label | contents |
| --- | --- | --- | --- | --- |
| sda1 | 2048 | 127 MiB | ONL-BOOT | `arm64-delta-tn48m-r0.itb` (28 MB), `boot-config` |
| sda2 | 262144 | 128 MiB | ONL-CONFIG | `pki/` (cert + key.pem), `identity-agent/` |
| sda3 | 524288 | 1 GiB | ONL-IMAGES | `DENTOS-master_ONL-OS9_2020-10-02.1819-afcdc85_ARM64.swi` |
| sda4 | 2621440 | 24.4 GiB | ONL-DATA | Debian 9 rootfs (`SWI=dir:data:/`) |

- `boot-config`: `NETDEV=eth0`, `BOOTMODE=INSTALLED`, `SWI=images::latest`.
- The ONL-BOOT FIT has a dentOS 5.6-lts kernel, DTBs for accton-as4224,
  delta-tn48m, delta-tx4810, delta-tn4810m, two NXP RDBs, and the ONL loader
  initrd. Its TN48M DTB is decompiled to `dentos-delta-tn48m.dts`, the best
  vendor description of the board we have.
- The rootfs kernel is `Linux 5.6.16 (admin@ip-172-31-28-137)`, built
  2020-10-14 on AWS.

## history: factory-provisioned for Amazon, never configured

- 2021-07-16: manufactured (EEPROM TLV).
- **2021-07-21**, at the DNI factory (Taiwan): ONL installed 08:55–08:59 (file
  times). It booted on a 10.10.0.0/24 test network (DHCP 10.10.0.5 from
  10.10.0.1), and `sshd` accepted a root password login from 10.10.0.1. onlpd
  saw fans 1-3 and **PSU 1 only**. Then the unit shut down. No later file
  writes exist anywhere on ONL-DATA, ONL-CONFIG or ONL-BOOT.
- Identity provisioning: `identity-agent/storage/persist/idme.properties` has
  `thing_type=Networking_Switch`, `psn=DENT_TN481N1TW21290148`,
  `serial=DENT_TN48M`, `asset_id=3811100345` (= the TLV service tag),
  `storage.engine.identity=hardware-tpm`. The identity key is a
  `TSS2 PRIVATE KEY`, wrapped by this unit's SLB9670 TPM. Its cert was issued
  2021-07-21 by Amazon's "Networking_Switch Provisioning CA (prod)", valid to
  2041. The trust store holds Amazon root CAs plus Amazon IHM/NEMO/ORVILLE/
  ECO/OPC/DPM provisioning CAs.
- That matches U-Boot's required FIT key `ihm_dent_image_signing`
  (`uboot.md`). This is an **Amazon DENT edge-switch SKU**, U-Boot included.
- ONL-DATA superblock: created 2021-07-21 08:56, **mount count 23, last
  mount/write 2022-05-11 04:47**, state clean. So something mounted it rw 22
  more times up to May 2022 without leaving any file changes. dentOS may keep
  runtime state in RAM, or something else did the mounting. **Unknown.**
- At some point after that, `nos_bootcmd` was set to `true`, so the box boots
  only to ONIE install mode. That's consistent with a decommission/resale
  wipe that left the disk.
- Configuration: hostname `localhost`, `/etc/network/interfaces` = lo + eth0
  DHCP, FRR default (`log syslog informational` only), empty `/home/cumulus`
  and `/home/system` skeletons, empty `/root/.history_frr`. **No customer
  config.**

Verdict: the switch is effectively **fresh**. Factory-provisioned, with no
deployment config persisted; the 22 later mounts are unexplained.

## hardware facts from the factory boot (`var/log/kern.log`)

- `Memory: 8074368K/8386560K`: the same 2 MiB psci-area hole as mainline.
- TPM: `tpm_tis_spi spi0.0: 2.0 TPM (device-id 0x1B, rev-id 16)` (SLB9670).
- I2C: CPLD `tn48m_cpld` @ 0x41 (bus 0), ADT7473 @ 0x2e
  ("Optional features: fan4 pwm2"), TMP1075 @ 0x4a and 0x4b (bus 1),
  4-port gpio i2c mux for the SFP cages. **No PMBus PSU devices instantiated.**
- Prestera: `AC3x B2B 98DX3265 0000:01:00.0`; firmware loaded and ready ~3 s
  after driver load.

## sensitive material (kept out of git)

`dumps/extracted/p2/` holds this unit's Amazon device identity: the CSR, the
cert chain, the TPM-wrapped key, and the ONL `pki/key.pem` (plain private key).
The TPM-wrapped key only works with this board's TPM. **Don't publish these,
and don't contact Amazon provisioning endpoints with them.** `dumps/.gitignore`
excludes `extracted/`, `*.img` and `*.zst`.
