# Trial boot with automatic revert (exported as nixosModules.profile-trial-boot).
#
# A trial generation is booted ONCE; the known-good generation stays the boot
# loader's persistent default and is what every other boot runs, including the
# one after a failed, panicking or hung trial. A trial is promoted only by an
# explicit `trial-boot promote` on the verified trial itself.
#
#   once per box:   <trial>/sw/bin/trial-boot setup
#   per trial:      <trial>/sw/bin/trial-boot stage <trial> [--append ARGS] [--reboot]
#   in the trial:   trial-boot-verify.service (kernel command line trialboot=<nonce>)
#   optionally:     trial-boot promote
#
# - Request: an 8-byte magic in one raw sector before the first partition (flag),
#   written by `stage`. The loader clears it (and reads it back) before it boots
#   the trial, so a trial is never booted twice.
# - Loader: a U-Boot script (/boot/boot.scr, ./boot-script.nix) that standard boot
#   runs before extlinux (the board makes that persistent: loader.setup). No
#   request: it does nothing (optionally disarms the board watchdog) and fails, so
#   standard boot goes on to extlinux.conf = the default. Request: clear it, arm
#   the board watchdog (loader.ubootArm), sysboot the trial's extlinux.conf on the
#   root fs (kernel, initrd, DTB straight from the store: nothing copied to
#   /boot). Any step failing boots the default.
# - Before userspace: the board watchdog armed by the loader (watchdog.bootTimeout)
#   and panic=10 on the trial's command line.
# - In the trial: trial-boot-verify kicks the watchdog and checks the management
#   network (carrier, IPv4 address, gateway, hosts) within verify.window. Pass:
#   disarm, stay up, not promoted. Fail: record why, re-arm, reboot (-> default).
# Results: /var/lib/trial-boot/history, `trial-boot status`. Board hooks
# (watchdog, flag sector, loader setting): boards/<board>/trial-boot.nix.
# Test: ./test.nix (this boot.scr in the real U-Boot on QEMU).
{ config, lib, pkgs, ... }:
let
  cfg = config.system.trialBoot;
  bootScr = import ./boot-script.nix { inherit lib pkgs; loader = cfg.loader; lba = cfg.flag.lba; };
  tool = pkgs.writeShellApplication {
    name = "trial-boot";
    runtimeInputs = with pkgs; [ coreutils util-linux gnugrep gnused gawk config.systemd.package ];
    text = ''
      FLAG_DEVICE=${lib.escapeShellArg cfg.flag.device}
      FLAG_LBA=${toString cfg.flag.lba}
      BOOT_SCR=${bootScr}/boot.scr
      SCRIPT_DEST=${lib.escapeShellArg cfg.loader.scriptPath}
      CONF_FILE=${lib.escapeShellArg cfg.loader.confFile}
      DTB_NAME=${lib.escapeShellArg cfg.dtbName}
      APPEND=${lib.escapeShellArg (lib.concatStringsSep " " cfg.append)}
      STATE=/var/lib/trial-boot
      loader_check() {
        ${cfg.loader.check}
      }
      loader_setup() {
        ${cfg.loader.setup}
      }
    '' + builtins.readFile ./trial-boot.sh;
  };
  verify = pkgs.writeShellApplication {
    name = "trial-boot-verify";
    excludeShellChecks = [ "SC2329" ];   # revert() and wd_arm() run from the EXIT trap
    runtimeInputs = with pkgs; [ coreutils iproute2 iputils gnugrep gnused gawk util-linux config.systemd.package ];
    text = ''
      IFACE=${lib.escapeShellArg cfg.verify.interface}
      HOSTS=${lib.escapeShellArg (lib.concatStringsSep " " cfg.verify.hosts)}
      CHECK_GATEWAY=${if cfg.verify.gateway then "1" else "0"}
      WINDOW=${toString cfg.verify.window}
      INTERVAL=${toString cfg.verify.interval}
      KICK_TIMEOUT=${toString cfg.watchdog.bootTimeout}
      REBOOT_TIMEOUT=${toString cfg.watchdog.rebootTimeout}
      STATE=/var/lib/trial-boot
      wd_kick() {
        ${cfg.watchdog.kick}
      }
      wd_arm() {
        ${cfg.watchdog.arm}
      }
      wd_disarm() {
        ${cfg.watchdog.disarm}
      }
    '' + builtins.readFile ./verify.sh;
  };
  str = lib.types.str;
  cmds = lib.types.listOf str;
in
{
  options.system.trialBoot = {
    enable = lib.mkEnableOption "one-shot trial boots with automatic revert (trial-boot CLI + verifier)";
    flag = {
      device = lib.mkOption { type = str; example = "/dev/sda"; description = "Whole disk with the one-shot request sector: the disk the loader boots from."; };
      lba = lib.mkOption { type = lib.types.ints.positive; example = 2047; description = "Request sector: before the first partition (stage checks), all zero when idle."; };
    };
    dtbName = lib.mkOption { type = str; default = ""; example = "amazon/alpine-v2-dmc12.dtb"; description = "DTB path under the trial toplevel's dtbs/ for its FDT line (empty: none)."; };
    loader = {
      scriptPath = lib.mkOption { type = str; default = "/boot/boot.scr"; description = "Where Linux installs the U-Boot script (standard boot finds it on the boot partition)."; };
      confPartition = lib.mkOption { type = lib.types.ints.positive; description = "Partition (on the boot disk) of the file system holding confFile."; };
      confPath = lib.mkOption { type = str; default = "/var/lib/trial-boot/extlinux.conf"; description = "confFile as U-Boot sees it on confPartition."; };
      confFile = lib.mkOption { type = str; default = "/var/lib/trial-boot/extlinux.conf"; description = "The trial's extlinux.conf as Linux sees it."; };
      ubootArm = lib.mkOption { type = cmds; default = [ "true" ]; description = "U-Boot commands (joined with &&) arming the board watchdog for watchdog.bootTimeout; must fail unless it is armed. \${tb_chk} is a scratch address."; };
      ubootDisarm = lib.mkOption { type = cmds; default = [ "true" ]; description = "U-Boot commands (&&) disarming it."; };
      ubootDisarmOnDefault = lib.mkOption { type = lib.types.bool; default = false; description = "Run ubootDisarm on boots without a request (a trial's SoC-only reset can leave a board watchdog armed)."; };
      check = lib.mkOption { type = lib.types.lines; description = "Shell: exit 0 if the loader runs the script before extlinux (a persistent loader setting)."; };
      setup = lib.mkOption { type = lib.types.lines; description = "Shell: make that setting (run by `trial-boot setup` when check fails)."; };
    };
    watchdog = {
      bootTimeout = lib.mkOption { type = lib.types.ints.positive; default = 180; description = "Seconds the loader arms the board watchdog for (loader to verifier start); the verifier kicks with the same value."; };
      rebootTimeout = lib.mkOption { type = lib.types.ints.positive; default = 120; description = "Seconds armed before rebooting a failed trial (backstop for a hung shutdown)."; };
      arm = lib.mkOption { type = lib.types.lines; default = "true"; description = "Shell: arm the board watchdog for $1 seconds."; };
      kick = lib.mkOption { type = lib.types.lines; default = "true"; description = "Shell: restart its countdown at $1 seconds."; };
      disarm = lib.mkOption { type = lib.types.lines; default = "true"; description = "Shell: disarm it."; };
    };
    verify = {
      interface = lib.mkOption { type = str; default = "eth0"; };
      hosts = lib.mkOption { type = lib.types.listOf str; default = [ ]; description = "Hosts that must answer ping (e.g. the workstation)."; };
      gateway = lib.mkOption { type = lib.types.bool; default = true; description = "The IPv4 default gateway must answer ping."; };
      window = lib.mkOption { type = lib.types.ints.positive; default = 300; description = "Seconds from verifier start for all checks to pass."; };
      interval = lib.mkOption { type = lib.types.ints.positive; default = 10; };
    };
    append = lib.mkOption { type = lib.types.listOf str; default = [ "panic=10" ]; description = "Added to every trial's kernel command line."; };
  };

  config = lib.mkIf cfg.enable {
    environment.systemPackages = [ tool ];
    systemd.services.trial-boot-verify = {
      description = "Trial boot: check the management network, else revert";
      wantedBy = [ "multi-user.target" ];
      after = [ "local-fs.target" "systemd-modules-load.service" ];
      before = [ "shutdown.target" ];
      conflicts = [ "shutdown.target" ];
      unitConfig = { DefaultDependencies = false; ConditionKernelCommandLine = "trialboot"; };
      serviceConfig = { Type = "simple"; ExecStart = "${verify}/bin/trial-boot-verify"; };
    };
    system.build.trialBoot = { inherit tool verify bootScr; };
  };
}
