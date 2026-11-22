# AS7772-32X: the two I2C2 (i2c@118100, Linux i2c-1) devices the AS7760 lacks.
# - 0x18 MAX1617A local + remote-diode sensor (kernel lm90 driver, already in
#   the shared T2080 kernel). Labelled for sensors(1); The remote diode follows CPU
#   load (2026-11-21): the T2080 die. fand uses it as sensor cpu_die (./default.nix).
# - 0x13 ST33ZP24 TPM 1.2 (TCG_TIS_ST33ZP24_I2C in the shared kernel) gives
#   /dev/tpm0 and /sys/class/tpm/tpm0. It is a 1.2 part: security.tpm2 and
#   tpm2-tools do not apply, and no TSS daemon is installed.
# DT nodes: packages/linux-qoriq-t2080/boards/as7772/as7772-delta.dtsi.
# Evidence: devices/edge-core-as7772-32x/tpm-sensor.md.
{ ... }:
{
  environment.etc."sensors.d/as7772-max1617.conf".text = ''
    chip "max1617-i2c-*-18"
      label temp1 "cpu_area_local"
      label temp2 "cpu_remote_diode"
  '';
}
