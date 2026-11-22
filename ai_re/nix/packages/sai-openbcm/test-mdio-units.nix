# VM test (no hardware) of the unit wiring around openbcm-mdio-script in
# modules/switch/openbcm.nix: hard fail (nothing that requires the PHY
# bring-up starts without it), what a failure leaves behind (status files,
# systemd status text, login message, phy-status, the temperature reader
# taking over), and the way back without a reboot.
# The module's real units run. What a VM cannot have is replaced by
# stand-ins that keep the one property the wiring is about, the per-unit SDK
# lock: a stand-in reader, a stand-in "syncd" and, once /var/lib/phy/sdk-works
# exists, a stand-in for a successful run all take /run/openbcm-unit0.lock
# without waiting and fail when it is held, as the SDK processes do. Without
# that file the real tool (SDK-less build) runs: a missing or malformed
# script is its real failure path, status files and all.
#   nix build --impure --expr 'let f = builtins.getFlake "path:<ai_re>/nix";
#     p = import f.inputs.nixpkgs { system = "x86_64-linux"; };
#     in import <ai_re>/nix/packages/sai-openbcm/test-mdio-units.nix { pkgs = p; }'
{ pkgs
, sonic ? import ../sonic { inherit pkgs; }
, libsai ? pkgs.callPackage ./. { inherit sonic; } }:
let
  lock = "/run/openbcm-unit0.lock";
  path = pkgs.lib.makeBinPath [ pkgs.coreutils pkgs.util-linux ];
  # bin/openbcm-mdio-script: the real tool, except for the simulated success.
  mdioScript = pkgs.writeShellScript "openbcm-mdio-script" ''
    PATH=${path}
    real=${libsai}/bin/openbcm-mdio-script
    [ -e /var/lib/phy/sdk-works ] || exec $real "$@"
    case " $* " in *" --verify-only "*) exec $real "$@" ;; esac
    for a in "$@"; do script=$a; done
    $real --check "$script" || exit 5
    exec 9>>${lock}
    flock -n 9 || { echo "another SDK process owns the switch"; exit 3; }
    echo run >> /var/lib/phy/runs
    out=; prev=
    for a in "$@"; do [ "$prev" = --temp-out ] && out=$a; prev=$a; done
    mkdir -p "$(dirname "$out")"
    for i in 1 2 3; do echo 40.$i > "$out.tmp"; mv "$out.tmp" "$out"; sleep 1; done
    rm -f "$out"
    echo "openbcm-mdio-script: ok: simulated" > /run/openbcm-mdio-script/status.txt
    echo '{"state": "ok"}' > /run/openbcm-mdio-script/status.json
  '';
  # bin/openbcm-asic-temp: writes --out every second while it holds the lock.
  asicTemp = pkgs.writeShellScript "openbcm-asic-temp" ''
    PATH=${path}
    out=; prev=
    for a in "$@"; do [ "$prev" = --out ] && out=$a; prev=$a; done
    exec 9>>${lock}
    flock -n 9 || { echo "openbcm-asic-temp: another SDK process owns the switch"; exit 3; }
    trap 'rm -f "$out"; exit 0' TERM INT
    while :; do echo 41.0 > "$out.tmp"; mv "$out.tmp" "$out"; sleep 1 & wait $!; done
  '';
  tools = pkgs.runCommand "sai-openbcm-vm-standins" { } ''
    mkdir -p $out/bin
    ln -s ${mdioScript} $out/bin/openbcm-mdio-script
    ln -s ${asicTemp} $out/bin/openbcm-asic-temp
  '';
  syncd = pkgs.writeShellScript "standin-syncd" ''
    PATH=${path}
    exec 9>>${lock}
    flock -n 9 || { echo "syncd: another SDK process owns the switch"; exit 3; }
    echo start >> /var/lib/phy/syncd-starts
    trap 'rm -f /run/asic/temperature; exit 0' TERM INT
    mkdir -p /run/asic
    while :; do echo 42.0 > /run/asic/temperature.tmp; mv /run/asic/temperature.tmp /run/asic/temperature; sleep 1 & wait $!; done
  '';
in
pkgs.testers.runNixOSTest {
  name = "openbcm-mdio-script-units";
  nodes.machine = { lib, pkgs, ... }: {
    imports = [ ../../modules/switch/openbcm.nix ];
    hardware.openbcm = {
      enable = true;
      package = tools;
      temperature = { enable = true; interval = 1; };
      mdioScript = {
        enable = true;
        files = [ "/var/lib/phy/init.mdio" ];
        sdkConfig = pkgs.writeText "config.bcm" "";
        requiredBy = [ "stack.target" "stack-syncd.service" ];
        stopOnFailure = [ "stack.target" ];
        phyNames = { "0x0" = "cage1"; "0x1" = "cage2"; };
        registerNames."1.8b00" = "id";
        hints.script-missing = "copy it with install-script.sh, then systemctl restart stack.target";
      };
    };
    # No switch chip here: no SDK kernel modules, no device nodes.
    boot.extraModulePackages = lib.mkForce [ ];
    boot.kernelModules = lib.mkForce [ ];
    boot.extraModprobeConfig = lib.mkForce "";
    systemd.services.openbcm-devices.script = lib.mkForce "true";
    systemd.tmpfiles.rules = [ "d /var/lib/phy 0700 root root -" ];
    # The shape of services.sonic: a target, syncd (owns the chip, displaces
    # the reader) and another member of the stack.
    systemd.targets.stack = { description = "stand-in switch stack"; wantedBy = [ "multi-user.target" ]; };
    systemd.services.stack-syncd = {
      wantedBy = [ "stack.target" ];
      partOf = [ "stack.target" ];
      conflicts = [ "openbcm-asic-temp.service" ];
      after = [ "openbcm-asic-temp.service" ];
      serviceConfig = { ExecStart = syncd; Restart = "always"; RestartSec = 1; };
    };
    systemd.services.stack-other = {
      wantedBy = [ "stack.target" ];
      partOf = [ "stack.target" ];
      serviceConfig.ExecStart = "${pkgs.coreutils}/bin/sleep infinity";
    };
    # As boards with SONiC do: the reader is not started at boot.
    systemd.services.openbcm-asic-temp.wantedBy = lib.mkForce [ ];
  };
  testScript = ''
    import json

    def state(unit):
        return machine.succeed(f"systemctl show -P ActiveState {unit}").strip()

    def wait_state(unit, want, timeout=60):
        machine.wait_until_succeeds(f"[ \"$(systemctl show -P ActiveState {unit})\" = {want} ]", timeout=timeout)

    def no_lock_clash():
        j = machine.succeed("journalctl -b --no-pager")
        assert "another SDK process owns the switch" not in j, "a stand-in found the SDK lock held: an ordering bug"

    SCRIPT = "mdio-script 1\\nL init\\nW 0 1 8201 7e\\nL verify\\nP 0 1 8b00 2391 ffff 1 0\\nP 1 1 8b00 2391 ffff 1 0\\n"

    machine.wait_for_unit("multi-user.target")

    with subtest("boot without the script: the bring-up fails, the stack does not start"):
        wait_state("openbcm-mdio-script.service", "failed")
        machine.wait_until_succeeds("systemctl is-active openbcm-asic-temp.service")
        wait_state("stack-other.service", "inactive")
        assert state("stack.target") == "inactive", state("stack.target")
        assert state("stack-syncd.service") == "inactive", state("stack-syncd.service")
        machine.fail("test -e /var/lib/phy/syncd-starts")
        print(machine.succeed("systemctl status openbcm-mdio-script.service stack.target stack-syncd.service --no-pager -l || true"))
        failed = machine.succeed("systemctl --failed --no-legend --plain | awk '{print $1}'").split()
        assert failed == ["openbcm-mdio-script.service"], failed

    with subtest("why: status files, systemd status text, journal, login message, phy-status"):
        st = json.loads(machine.succeed("cat /run/openbcm-mdio-script/status.json"))
        assert st["state"] == "failed" and st["failure"]["class"] == "script-missing", st
        assert "install-script.sh" in st["failure"]["hint"], st
        txt = machine.succeed("cat /run/openbcm-mdio-script/status.txt")
        assert txt.startswith("openbcm-mdio-script: FAILED (script-missing): /var/lib/phy/init.mdio: No such file or directory; nothing was written to any PHY"), txt
        status = machine.succeed("systemctl show -P StatusText openbcm-mdio-script.service")
        assert "FAILED (script-missing)" in status, status
        j = machine.succeed("journalctl -b --no-pager -u openbcm-mdio-script -u openbcm-mdio-script-failed")
        assert "what to do: copy it with install-script.sh" in j and "not starting: stack.target stack-syncd.service" in j, j
        login = machine.succeed("bash -lic true 2>&1")
        assert "*** PHY bring-up failed; not started: stack.target stack-syncd.service ***" in login and "FAILED (script-missing)" in login and "phy-status" in login, login
        # nothing on the output of a command run over ssh (scp, nix copy, nixos-rebuild)
        assert machine.succeed("bash -lc true 2>&1").strip() == "", "the login message must be for interactive shells only"
        ps = machine.succeed("phy-status")
        assert "FAILED (script-missing)" in ps and "syncd is not running" in ps, ps
        assert json.loads(machine.succeed("phy-status --json"))["state"] == "failed"

    with subtest("the reader took over the ASIC temperature"):
        machine.wait_until_succeeds("grep -q 41.0 /run/asic/temperature")

    with subtest("starting the stack again while the cause is still there fails again, and the reader comes back"):
        machine.fail("systemctl start stack.target")
        wait_state("openbcm-mdio-script.service", "failed")
        machine.wait_until_succeeds("systemctl is-active openbcm-asic-temp.service && grep -q 41.0 /run/asic/temperature")
        assert state("stack-syncd.service") == "inactive"
        wait_state("stack-other.service", "inactive")

    with subtest("a damaged script is its own class, and still nothing starts"):
        machine.succeed("printf 'mdio-script 1\\nW 0 1 8201\\n' > /var/lib/phy/init.mdio")
        machine.fail("systemctl restart stack.target")
        wait_state("openbcm-mdio-script.service", "failed")
        st = json.loads(machine.succeed("cat /run/openbcm-mdio-script/status.json"))
        assert st["failure"]["class"] == "script-malformed", st
        machine.wait_until_succeeds("systemctl is-active openbcm-asic-temp.service")
        no_lock_clash()

    with subtest("cause fixed: systemctl restart of the stack runs the bring-up and starts everything, no reboot"):
        machine.succeed(f"printf '{SCRIPT}' > /var/lib/phy/init.mdio", "touch /var/lib/phy/sdk-works")
        machine.succeed("systemctl restart stack.target")
        assert state("openbcm-mdio-script.service") == "active"
        machine.wait_until_succeeds("systemctl is-active stack-syncd.service stack-other.service stack.target")
        machine.succeed("[ $(wc -l < /var/lib/phy/runs) = 1 ]")
        # the reader was stopped for the run (ExecStartPre) and stays stopped under syncd; syncd publishes
        machine.wait_until_succeeds("grep -q 42.0 /run/asic/temperature")
        assert state("openbcm-asic-temp.service") == "inactive", state("openbcm-asic-temp.service")
        assert machine.succeed("cat /var/lib/phy/syncd-starts").count("start") == 1
        login = machine.succeed("bash -lic true 2>&1")
        assert "PHY bring-up failed" not in login, login
        assert machine.succeed("systemctl --failed --no-legend --plain").strip() == ""
        no_lock_clash()

    with subtest("a stack restart does not run the bring-up again"):
        machine.succeed("systemctl restart stack.target")
        machine.wait_until_succeeds("systemctl is-active stack-syncd.service")
        machine.succeed("[ $(wc -l < /var/lib/phy/runs) = 1 ]")
        assert machine.succeed("cat /var/lib/phy/syncd-starts").count("start") == 2

    with subtest("restarting the bring-up by hand takes its dependents down and up around it"):
        machine.succeed("systemctl restart openbcm-mdio-script.service")
        machine.succeed("[ $(wc -l < /var/lib/phy/runs) = 2 ]")
        machine.wait_until_succeeds("systemctl is-active stack-syncd.service stack.target")
        no_lock_clash()

    with subtest("reboot with the script in place: straight up, once"):
        machine.succeed("rm /var/lib/phy/runs /var/lib/phy/syncd-starts; sync")
        machine.shutdown()
        machine.start()
        machine.wait_for_unit("multi-user.target")
        machine.wait_until_succeeds("systemctl is-active stack-syncd.service stack-other.service stack.target")
        machine.succeed("[ $(wc -l < /var/lib/phy/runs) = 1 ]")
        assert state("openbcm-asic-temp.service") == "inactive"
        assert machine.succeed("systemctl --failed --no-legend --plain").strip() == ""
        no_lock_clash()
  '';
}
