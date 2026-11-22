# AS7772-32X: TPM (0x13) and thermal sensor (0x18) on I2C2

Both sit on the T2080's second I2C controller, `soc/i2c@118100` = Linux `i2c-1` (vendor CSV "I2C2", bus index 1).
The AS7760's `pca9546@77` on this bus does not exist here (0x77 NAKs). Deployed 2026-11-21; both drivers bind,
0 failed units.

| addr | part | state | evidence |
| --- | --- | --- | --- |
| 0x18 | **MAX1617A**-compatible local + remote-diode sensor (8-bit, 1 C); lm90 binds as `max1617` | 24 C local / 32 C remote at idle. Remote follows CPU load (8 busy loops for 75 s: 32 -> 40 C, local 24 -> 25 C, back to 36 C in 30 s): the **T2080 die diode** in all likelihood. Not a fand input in the committed tree | live read: manufacturer ID (0xfe) 0x4d, device ID (0xff) 0x01, config 0x00, conv rate 0x02, limits 127 / -55; vendor DT `ne1617a@18`, `"nxp,max1617a"`. Maxim part or NXP NE1617A clone: not known (marking not read) |
| 0x13 | **ST33ZP24** TPM **1.2**, ST's own I2C protocol (not TCG TIS-I2C) | `/dev/tpm0`, caps "Manufacturer 0x53544d20 (STM), TCG 1.2, firmware 13.10"; **enabled 0, active 0, owned 0, temp_deactivated 1**, left as found | driver binds at boot; vendor DT `tpm@13`, `"st33,st33zp24_i2c"`, vendor kernel has `tpm_i2c_stm_st33`. No interrupt or LPCPD GPIO in the vendor DT: polled |

- Probe transcript (read-only byte reads at 0x13/0x18/0x77): `evidence/tpm-sensor-probe-20261121.txt` (sha256
  cd6baecd...7409). Reading the MAX1617 status register (0x02) clears latched alarms; none were set.
- The ONIE scan also shows an ACK at **0x1b** on this bus (and 0x1a/0x1b on bus 0): not in the vendor map, not probed.

## What was added
- Kernel (shared T2080 kernel, `nix/packages/linux-qoriq-t2080/default.nix`): `TCG_TPM=y`,
  `TCG_TIS_ST33ZP24_I2C=y` (`SENSORS_LM90=y` was there). The AS7760 gets the same kernel (intended, the driver
  binds only to a DT node): kernel drv `5nldvdb9...` -> `9sdiyc9w...`. Not enabled: `HW_RANDOM_TPM`, `TCG_TPM2_HMAC`.
- DT (`boards/as7772/as7772-delta.dtsi`): `tpm@13 "st,st33zp24-i2c"`, `temperature-sensor@18 "maxim,max1617"`
  (lm90 has no OF entry; the I2C core matches the `max1617` id), `/delete-node/ pca9546@77`.
- NixOS (`nix/modules/boards/as7772/tpm-sensor.nix`): sensors.d labels `cpu_area_local` / `cpu_remote_diode`.

## Open
- Enabling the TPM needs TPM 1.2 physical-presence commands (owner's call). No TSS is installed: it is a 1.2
  part, so `security.tpm2` / tpm2-tools do not apply; the trousers / tpm-tools ppc64 BE cross-build is unchecked.
