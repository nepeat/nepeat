# AS7772: Rust-enabled kernel + Rust as7760-fan on hardware (task Y, 2026-11-23, times PST)

Base `631c5hx7kznld417zvpvp38w7ml2dc05` (47c0012, attr as7772: Rust SAI, C fan driver, kernel without the
series; profile generation 30). Trial `na3yx1c2yqyaf5vi1lk7n915aylwp1a1` = base.extendModules
{ hardware.qoriqT2080.rust = true; hardware.as7760Fan.driver = "rust"; } (generation 31, built by
the scratchpad trial-build.sh from the git flake at that rev; closure diff: kernel 8paw4bmx, its modules,
openbcm-kmods ma0q29k6 rebuilt, initrd/uImage/uInitrd, fan driver as7760-fan-rs dx9ksx1x). Installed with
nix copy + nix-env --set + switch-to-configuration boot; fans held at 100 % (fand stopped) for each reboot.

| step | result |
| --- | --- |
| a0/a1 base | 0 failed units, C as7760_fan bound, 32/32 PHYs; boot log has 3 iommu WARNINGs + 2 PCIe window notes (taint W, baseline) |
| b0 install | extlinux: Default + 31 (trial), 30 (base), 29, 28 |
| c0 first boot | CONFIG_RUST=y, Rust as7760_fan (modinfo "(Rust)") bound to 4-0066, fand on its curve |
| c1 | MKA up on 4 ports by itself, slowest 15.3 s |
| c2 | 32 of 32 PHYs as the script expects |
| c3 ABI module | e6500, 8 CPUs: RESULT pass=338 fail=0, twice; only the module's own warn_on |
| c4/c5 | sonic-traffic-test, macsec-traffic-test: 0 % loss everywhere, encrypted = accepted |
| c6 pwm writes | 200->204, 128/129->136, 254/255->255, 170->170; 100/127/256/-1/abc EINVAL (= C driver in QEMU) |
| c7 sysfs | 27 lines (device, driver/module, each attribute's mode, labels, faults, pwm1 mode) identical to the C driver's |
| c8/c9 dmesg | only lines not in the base boot: the ABI module's two warn_on |
| d0 second boot | trial again, MKA up (14.8 s), 32/32 PHYs |
| e0 soak | loadtest/sonic-ring.sh through SONiC with MACsec, 17:10-19:14 UTC (2 h 3 min), 26 samples: 1.30e11 frames per port and direction, 17.7 Mpps / 92.1 Gb/s per port, 0 discards / errors before the stop, MACsec INVALID/NOT_VALID/LATE 0, 8 SAs open, ASIC 46.7-48.1 C, fans 13,000-17,100 rpm, fand never failsafe |
| e1 after soak | 32/32 PHYs, MKA PASS, no new kernel messages |

End state: the box runs and boots the trial (generation 31); the base (30) stays in the boot menu as rollback.
Console log: serial-*-console.txt (DMC-12 port 48).

| file | sha256 (16) |
| --- | --- |
| a0-base-state-fans.txt | 7bf8245bf778db9b |
| a1-base-dmesg.txt | a615a0a0db125e2f |
| b0-install-trial-boot.txt | 488e90b885624704 |
| c0-trial-first-state-fans.txt | 1a52771cadd318b5 |
| c1-trial-macsec-up-check.txt | 7921b6ed99ce6c90 |
| c2-trial-phy-status.txt | bb02b4f87d0efbe9 |
| c3-trial-abi-module-e6500.txt | 91cf50ec6ff30238 |
| c4-trial-sonic-traffic-test.txt | 6cffd0d104880a88 |
| c5-trial-macsec-traffic-test.txt | 8614e99fd20d9410 |
| c6-trial-fan-pwm-writes.txt | fcdd9e2e75dbdf8d |
| c7-fan-sysfs-parity.txt | 2c4b7c0f8e6270d8 |
| c8-trial-dmesg.txt | 7ad36599fb09bada |
| c9-dmesg-diff-vs-base.txt | 81a0b5192d343d6e |
| d0-trial-reboot2-state.txt | 16575bf5d19d0b30 |
| e0-trial-soak-sonic-ring-2h.txt | 972e435394f76324 |
| e1-trial-after-soak-state.txt | 09c8147789603605 |
| serial-20261123T1654Z-console.txt | 8c774cbfe0a1c15a |
