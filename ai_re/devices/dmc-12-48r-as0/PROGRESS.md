# DMC-12-48R-AS0 serial console server

## Status
- New for review. Nothing probed yet: no serial, network, or teardown.
- Power: HA lab plug 2, `lab-power 2 status|on|off`
  (`switch.server_room_lab_plug_2`). Plug 1 is the lab switch another agent
  is using; don't touch it from here.
- Left **off** after the 2026-11-15 plug test.
- Open: identity (make, board, SoC, flash), input voltage, console port and
  baud, network defaults.

## Identity
- Model: DMC-12-48R-AS0 (serial terminal server). Make, part numbers, chip
  markings: TBD.

## Power
- Mains via HA lab plug 2. Input voltage / PSU: TBD.
- Logic level: TBD, record before probing any header.

## Interfaces
- TBD.

## Log (newest first)
- 2026-11-15: `lab-power` now needs a plug number. Switched plug 2 on, then
  off (02:10:29–02:10:36 PST); HA confirmed each state. Plug 1 stayed `on`
  throughout (its `last_changed` 09:19:30Z predates the test). Only HA's
  reported state was checked, not whether the unit actually powered up.
