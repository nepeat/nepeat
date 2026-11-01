# UART recovery

A report for this exact model identifies a 4-pin TTL connector inside the
housing. The board has not yet been inspected, so its pin order, voltage, baud
rate, parity, and framing remain unknown.

A read-only dump of this unit's bootloader confirms an active UART recovery
console. It prints `Hit Ctrl+u to stop autoboot:`, uses the `HKVS #` prompt, and
offers `erase`, `show ptb`, and `reset`. The kernel command line contains
`console=ttyS0`. These findings make 115200 8N1 the first sensible capture
setting, but the baud rate still must be confirmed from readable output.

## First connection

1. Disconnect PoE and 12 V power before opening the housing.
2. Remove the top sun cover, then the two top screws holding the front cover. Lift off the front cover.
3. Remove the four recessed screws at the front. Separate the front and rear sections carefully; preserve the rubber seal and do not strain the ribbon cable.
4. Photograph the board and connector before moving any cables or screws.
5. Identify ground with continuity to the RJ45 shield or a large ground plane while unpowered.
6. Power the camera and measure every remaining pin relative to ground with a multimeter. Record the idle voltages before attaching an adapter.
7. Connect adapter ground and camera TX to adapter RX only. Do not connect the adapter's VCC pin. Start with a read-only boot capture.
8. Try 115200 8N1 first, then determine the actual framing from the boot output rather than assuming it.

The host currently has no USB serial adapter attached. Once a safe receive-only connection exists, use a dedicated tmux session rather than the user's existing tmux session `0`:

```sh
tmux new-session -d -s hikvision-serial \
  'tio --log-file devices/hikvision-DS-2CD2T22WD-I5/captures/uart-boot.txt /dev/ttyUSB0 -b 115200'
```

After a clean boot log confirms the console and bootloader, connect adapter TX
to camera RX. Prefer inspecting U-Boot environment and flash layout before
changing variables or writing flash. The configuration-only reset procedure is
documented in [password-reset.md](password-reset.md).

Reference: [exact-model recovery discussion](https://ipcamtalk.com/threads/ds-2cd2t22wd-i5-bricked.51634/)

Housing instructions: [DS-2CD2T42WD-I5 teardown](https://www.jamescooke.me.uk/diy-home-life/hikvision-ip-camera-stuck-on-black-white-night-mode-fixed/)
