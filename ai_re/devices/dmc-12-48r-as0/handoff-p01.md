# Handoff: finish deploying dma-con-sw-0 (p01) — for a machine that reaches p01 and dreamflasher

State as of 2026-11-16 ~04:30 PST. p01 = `dma-con-sw-0`, the production DMC-12 console switch
(root@100.73.84.99 on tailscale, `dma-con-sw-0.wt0.dma.computer` on netbird). Builder =
`dreamflasher.wt0.dma.computer` (aarch64). GitOps source = **https://git.dma.space/infra/nixos.git**
`main` (not codeberg). **Do not reboot p01 without someone watching its serial console** (step 6).

## Where it stands
- p01 runs generation **31 = `…nixos-system-dma-con-sw-0-rev-e9f4a8b-…`** (switched 04:18 by a manual
  `nixos-rebuild`): our board module from `github:nepeat/nepeat?dir=ai_re` (fand instead of the old
  `dmc12-fancontrol`, netbird logs in a bounded journal namespace, fixed comin config). Booted kernel is
  still the old 7.2.3 (`/run/booted-system` = `…rev-62c107f…`); the new 7.3 kernel/DTB apply at reboot.
- Forge `main` = **`e56b987`** (dmc12 switch + netbird fix rebased onto today's nixpkgs bumps). comin was
  started at 04:26 and is evaluating it. The nixpkgs bump means a **new kernel** derivation
  (`/nix/store/dns7igqqvadf9idmi2w3307ca44m367l-linux-7.3.4.drv`) that nothing has built yet.
- **p01 cannot reach dreamflasher** (netbird: no ping, tcp/22 times out; 5/19 peers connected), so comin's
  remote build fails and nix falls back to compiling the kernel on p01 (4x A57, 3.9 GB RAM: hours,
  past comin's 2 h build_timeout). That is the main thing to avoid.
- Known failing units after the switch: `prometheus-node-exporter` (repeated `--collector.systemd`; fixed in
  monorepo `fe46e5b`, not yet on GitHub / in the lock) and `tty-sysinfo-gitops` (infra's own jq error on
  comin's status). Everything else (fand, comin, netbird-wt0, tailscaled, sshd) was active.
- Disk: root 60% after GC, /boot 26%. comin state backup: `/var/lib/comin.bak-20261116/store.json`.

## Steps
1. **Stop p01 from compiling the kernel itself** (if comin is already building `linux-7.3.4` there):
   `ssh root@p01 'systemctl stop comin'`. (comin shows `failed` after a stop mid-build; harmless.)
2. **Build on dreamflasher** (native aarch64, fast):
   ```sh
   ssh root@dreamflasher 'nix build --print-out-paths --no-link \
     "git+https://git.dma.space/infra/nixos.git?rev=e56b987967928a6e3da3bdef232a61ec5e79f6e4#nixosConfigurations.dma-con-sw-0.config.system.build.toplevel"'
   ```
   It must end in `…-rev-e56b987-…`. Use the commit comin evaluates (check its log) if main moved.
3. **Relay the closure to p01** (they can't talk to each other; this machine can talk to both):
   ```sh
   ssh root@p01 'nix-store --gc | tail -1; df -h /'          # room first (closure ~3 GB)
   nix copy --no-check-sigs --from ssh-ng://root@dreamflasher.wt0.dma.computer \
            --to ssh-ng://root@p01 /nix/store/<toplevel from step 2>
   ```
   `--no-check-sigs` is needed (locally built paths are unsigned) and fine as root (trusted).
4. **Let comin deploy**: `ssh root@p01 'systemctl reset-failed comin; systemctl start comin; journalctl -fu comin'`.
   It should evaluate `e56b987`, find everything valid, and `switch`. Check:
   `readlink /run/current-system` ends `rev-e56b987`; `systemctl is-active fand comin netbird-wt0`.
   Fallback if comin misbehaves: the manual switch that worked (note the PATH: nixos-rebuild-ng calls `test`):
   ```sh
   systemd-run --unit=nixos-rebuild-bootstrap --collect --property=TimeoutStartSec=infinity \
     --setenv=PATH=/run/wrappers/bin:/run/current-system/sw/bin --setenv=HOME=/root \
     /run/current-system/sw/bin/nixos-rebuild switch -L --option builders "" --max-jobs 1 --cores 4 \
     --flake "git+file:///var/lib/comin/repository?ref=main&rev=e56b987967928a6e3da3bdef232a61ec5e79f6e4#dma-con-sw-0"
   ```
   (Exit status 4 from switch-to-configuration = activated but units failed: see the known failures above.)
5. **Fix node_exporter** (owner): push monorepo `fe46e5b` to GitHub, then in infra
   `nix flake update nepeat-nixre`, push to the forge `main`; comin redeploys (small, no kernel change).
6. **Before the first reboot** (owner decision, nothing done yet):
   - The boot menu's fallback is "Gen 30" = the **August** system (`rev-1ef6c57`, kernel 7.1.3). The October
     system that ran 41 days (`/nix/store/2j4pvma0xzfpv80g4r92mm90cjmxvpsl-nixos-system-dma-con-sw-0-rev-62c107f-26.11.20260907.dc5d91f`)
     was never a profile generation and is only kept alive as `/run/booted-system`. To keep it as a menu entry:
     ```sh
     new=$(readlink -f /run/current-system)
     nix-env -p /nix/var/nix/profiles/system --set /nix/store/2j4pvma0xzfpv80g4r92mm90cjmxvpsl-nixos-system-dma-con-sw-0-rev-62c107f-26.11.20260907.dc5d91f
     nix-env -p /nix/var/nix/profiles/system --set "$new"          # default = newest generation again
     "$new"/bin/switch-to-configuration boot                       # rewrites /boot/u-boot.menu
     head -8 /boot/u-boot.menu                                     # default = new, and the 62c107f entry listed
     ```
   - Reboot only with p01's serial console watched: first boot anywhere of factory U-Boot 2015.07 +
     lean 7.3 kernel + our DTB (al-fic USB interrupts, SPI TPM, SPI flash read-only). If it fails, pick the
     previous entry in the 5 s bootmenu. Static checks passed (load addresses: kernel 0x10000000 -> 0x04200000,
     initrd 0x20000000, DTB 0x04078000; kernel params and NIC name `en*` unchanged); a full test build passed.
   - After it boots: `uname -r` = 7.3.4; `grep hcd /proc/interrupts` counts up (the USB fix);
     `cat /proc/mtd` lists spi0-boot/u-boot-env/spi0-tail/spi1-alt; `ls /sys/class/tpm/tpm0`; 48 `/dev/ttyUSB*`;
     `systemctl is-active fand`. Then `fw-inventory.py collect --unit p01 …` (monorepo) can read its SPI.
7. **Fix p01 -> dreamflasher over netbird**, or future deploys that need real builds will hit this again.

## Don't
- Don't run `dmc12-uboot-env set` on p01: it targets our U-Boot's env window; p01 has the factory U-Boot.
- Don't write to I2C bus 4 address 0x10 (the CPLD's config port).
- `pkill -f PATTERN` over ssh kills your own remote shell when its command line contains PATTERN; pipe
  scripts with `ssh host sh -s <<EOF` instead.
- Don't leave a runtime comin override in `/run/systemd/system/comin.service.d/` (the bootstrap one is removed).

Later cleanup: `/var/lib/comin.bak-20261116` once comin has deployed on its own.
Background: `units.md` "Production units", PROGRESS.md 2026-11-16 entries.
