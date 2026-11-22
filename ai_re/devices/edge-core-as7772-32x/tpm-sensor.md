# AS7772-32X: TPM (0x13) and thermal sensor (0x18) on I2C2

Both sit directly on the T2080's second I2C controller, `soc/i2c@118100` = Linux `i2c-1`. The vendor CSV calls it
"I2C2", bus index 1 (`i2c_map-T2080-i2c2.csv`; "I2C1 0x13" in older notes means bus index 1, not controller 1).
The AS7760's `pca9546@77` on this bus does not exist here (0x77 NAKs, `pca954x 1-0077: probe failed`).

State 2026-11-21: **deployed and booted; both drivers bind.** TPM: `/dev/tpm0`, `tpm_version_major` 1, caps
"Manufacturer 0x53544d20 (STM), TCG 1.2, firmware 13.10"; **enabled 0, active 0, owned 0, temp_deactivated 1**
(dmesg: "TPM is disabled/deactivated (0x7)"), left as found: enabling needs TPM 1.2 physical-presence commands
(owner's call), and no TSS is installed. Sensor: lm90 bound as `max1617`, 24 C local / 32 C remote at idle; the
`pca954x 1-0077` probe error is gone. 0 failed units, fand unchanged. The remote diode follows CPU load (8 busy loops for 75 s: 32 -> 40 C, local 24 -> 25 C, back to 36 C within 30 s), so it is the **T2080 die diode** in all likelihood; not yet a fand input.

## Identification

| Addr | Part | Evidence | Confidence |
|---|---|---|---|
| 0x18 | **MAX1617A**-compatible local + remote-diode sensor (8-bit, 1 C) | Live read: manufacturer ID (0xfe) **0x4d** (Maxim), device ID (0xff) **0x01**; config 0x00, conv rate 0x02, limits 127 / -55 (power-on defaults), registers 0x09-0x1f all 0x01. This is exactly the pattern `lm90.c` detects as `max1617`. Local 26 C, remote 35 C. Vendor diag DT: `ne1617a@18`, `"nxp,max1617a"`. | ID verified by read. Whether the silicon is a Maxim MAX1617A or an NXP NE1617A clone that reports the same IDs is not known (marking not read). What the remote diode measures is **not verified** (the T2080 die diode is the likely candidate; it read 9 C above local at idle). |
| 0x13 | **ST33ZP24** TPM **1.2**, ST's own I2C protocol (not TCG TIS-I2C / PTP) | Vendor diag DT: `tpm@13`, `"st33,st33zp24_i2c"`; vendor kernel 3.12.19 contains `tpm_i2c_stm_st33` ("TPM I2C Initialized", `tpm_st33_i2c_probe`); the diag's `tpm` recipe runs `/usr/local/sbin/tpm-test`. Live: 0x13 ACKs, receive-byte returns 0xff. | **Confirmed at boot by the driver binding** (2026-11-21, state line above). Before that, no ID was read: SMBus read-byte-data at 0x80/0x94/0x98 NAKs (expected, the protocol is write `[reg|0x80, dummy]`, stop, read, which i2cget cannot do, and I did not use i2ctransfer writes). The driver probe at boot is the real test. Vendor DT gives no interrupt and no LPCPD GPIO, so the node is polled. |

Probe transcript (i2cdetect -r, i2cdump/i2cget byte reads only, bus 1, addresses 0x13/0x18/0x77 only):
[tpm-sensor-probe-20261121.txt](tpm-sensor-probe-20261121.txt) (sha256 cd6baecd...7409). Side effect: reading the
MAX1617 status register (0x02) clears latched alarm flags; none were set (0x00, then 0x80 = busy).
The ONIE scan also shows an ACK at **0x1b** on this bus (and 0x1a/0x1b on bus 0); not in the vendor map, not probed.

## What was added

- Kernel (shared T2080 kernel, `nix/packages/linux-qoriq-t2080/default.nix`): `TCG_TPM=y`, `TCG_TIS_ST33ZP24_I2C=y`.
  The sensor needs nothing new: `SENSORS_LM90=y` was already there. This changes the AS7760's kernel too
  (intended, the driver only binds to a DT node): `as7760-kernel` drv `5nldvdb9...-linux-...-7.3.4.drv` ->
  `9sdiyc9w...-linux-...-7.3.4.drv`; `as7760-system` drv `20axlar4...` -> `39i9md2c...` (the latter may include
  other agents' concurrent edits). Not enabled: `HW_RANDOM`/`HW_RANDOM_TPM` (TPM as hwrng), `TCG_TPM2_HMAC`.
- DT (`boards/as7772/as7772-delta.dtsi`): `tpm@13 "st,st33zp24-i2c"`, `temperature-sensor@18 "maxim,max1617"`
  (lm90 has no OF entry for it; the I2C core matches the `max1617` id), `/delete-node/ pca9546@77`.
- NixOS (`nix/modules/boards/as7772/tpm-sensor.nix`): sensors.d labels `cpu_area_local` / `cpu_remote_diode`.
  Not a fand input (remote diode unverified; the board zone curve would react to a 35 C reading).
  No userspace TPM stack: it is a 1.2 part, so `security.tpm2`/tpm2-tools do not apply; trousers/tpm-tools were
  not added and their ppc64 BE cross-build was **not checked**.

## Checks after boot
Done 2026-11-21 (results in the state line above): dmesg, `/sys/class/tpm/tpm0/{caps,enabled,active,owned}`,
`sensors 'max1617-*'`, driver link of `1-0018`, `systemctl --failed`. Not done: if the TPM is ever enabled, `pcrs` and a TSS (trousers/tpm-tools cross-build unchecked).
