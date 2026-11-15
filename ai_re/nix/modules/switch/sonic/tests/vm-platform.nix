# NixOS VM test of the platform side of services.sonic (virtual SAI): the
# pmon daemons over the AS7760 sonic_platform on a fake board
# (boards/as7760/fake_board.py: hwmon, fand state, ASIC temperature, hw-info
# MTD, CPLD#1 and two module EEPROMs as files), the show platform /
# transceiver CLI, hot-plug, fan control untouched, and LLDP on a self-loop:
# Ethernet5/1 and Ethernet6/1 sit on the same VM network, as the lab's
# Ethernet1/1 <-> Ethernet32/1 cable, so the switch sees itself.
#   nix build -L --impure --expr 'let f = builtins.getFlake "path:<ai_re>/nix";
#     pkgs = import f.inputs.nixpkgs { system = "x86_64-linux"; };
#     in import <ai_re>/nix/modules/switch/sonic/tests/vm-platform.nix { inherit pkgs; }'
{ pkgs }:
let
  sonic = import ../../../../packages/sonic { inherit pkgs; };
  platformPackage = pkgs.callPackage ../../../boards/as7760/sonic-platform-package.nix { inherit sonic; };
  fakeRoot = "/var/lib/as7760-fake";
  pmonUnits = [ "xcvrd" "thermalctld" "psud" "syseepromd" ];
in
pkgs.testers.runNixOSTest {
  name = "sonic-platform-lldp";
  nodes.switch = { lib, pkgs, ... }: {
    imports = [ ./common.nix ];
    virtualisation.memorySize = 3072;
    virtualisation.cores = 2;
    # eth1 (Ethernet5/1) and eth2 (Ethernet6/1) on the same network: a loop.
    virtualisation.vlans = [ 1 1 ];
    networking.interfaces = lib.genAttrs [ "eth1" "eth2" ] (_: {
      ipv4.addresses = lib.mkForce [ ];
      ipv6.addresses = lib.mkForce [ ];
    });
    networking.firewall.enable = false;
    services.sonic.settings.PORT = { "Ethernet5/1".admin_status = "up"; "Ethernet6/1".admin_status = "up"; };
    services.sonic.pmon = {
      enable = true;
      inherit platformPackage;
      environment.AS7760_PLATFORM_ROOT = fakeRoot;
    };
    services.sonic.lldp.enable = true;
    systemd.services = {
      fake-as7760 = {
        description = "Fake AS7760 hardware for sonic_platform";
        wantedBy = [ "sonic.target" ];
        serviceConfig = { Type = "oneshot"; RemainAfterExit = true; };
        script = "rm -rf ${fakeRoot}; ${pkgs.python3}/bin/python3 -I ${../../../boards/as7760/fake_board.py} ${fakeRoot}";
      };
    } // lib.genAttrs (map (d: "sonic-${d}") pmonUnits) (_: {
      requires = [ "fake-as7760.service" ];
      after = [ "fake-as7760.service" ];
    });
    environment.systemPackages = [ pkgs.jq ];
  };

  testScript = ''
    import json, re
    ROOT = "${fakeRoot}"
    FAILS = []

    def check(name, cond, detail=""):
        d = str(detail)
        print(("PASS " if cond else "FAIL ") + name + ("" if cond else " :: " + d[-3000:]))
        if not cond:
            FAILS.append(name)

    def sh(cmd):
        return switch.execute(cmd)

    def cli(cmd):
        return sh("bash -lc " + json.dumps(cmd + " 2>&1"))[1]

    def db(n, cmd):
        return sh("redis-cli -s /var/run/redis/redis.sock -n %d %s" % (n, cmd))[1].strip()

    def wait(name, cmd, timeout=180):
        try:
            switch.wait_until_succeeds(cmd, timeout=timeout)
            check(name, True)
        except Exception as e:
            check(name, False, e)

    start_all()
    switch.wait_for_unit("sonic-orchagent.service", timeout=300)
    switch.wait_until_succeeds("redis-cli -s /var/run/redis/redis.sock -n 0 exists PORT_TABLE:PortInitDone | grep -q 1", timeout=240)
    for u in ["fake-as7760", "sonic-xcvrd", "sonic-thermalctld", "sonic-psud", "sonic-syseepromd",
              "sonic-lldpd", "sonic-lldpmgrd", "sonic-lldp-syncd"]:
        switch.wait_for_unit(u + ".service", timeout=120)
    pwm_before = sh("stat -c '%Y %s' " + ROOT + "/sys/class/hwmon/hwmon0/pwm1; cat " + ROOT + "/sys/class/hwmon/hwmon0/pwm1")[1]

    # --- transceivers (xcvrd -> STATE_DB -> sfpshow)
    wait("xcvrd: TRANSCEIVER_INFO Ethernet0 (SFP28 DAC in a QSA)",
         "redis-cli -s /var/run/redis/redis.sock -n 6 hget 'TRANSCEIVER_INFO|Ethernet0' manufacturer | grep -q FAKE-VENDOR")
    wait("xcvrd: TRANSCEIVER_INFO Ethernet124 (QSFP28 SR4)",
         "redis-cli -s /var/run/redis/redis.sock -n 6 hget 'TRANSCEIVER_INFO|Ethernet124' model | grep -q FAKE-QSFP28-SR4")
    out = cli("show interfaces transceiver presence")
    print(out)
    check("presence: Ethernet1/1 Present", any(l.split()[:2] == ["Ethernet1/1", "Present"] for l in out.splitlines()), out)
    check("presence: Ethernet32/1 Present", any(l.split()[:2] == ["Ethernet32/1", "Present"] for l in out.splitlines()), out)
    check("presence: Ethernet2/1 Not present", any(l.split()[:3] == ["Ethernet2/1", "Not", "present"] for l in out.splitlines()), out)
    out = cli("show interfaces transceiver eeprom Ethernet32/1")
    print(out)
    check("eeprom Ethernet32/1: vendor, part, 100GBASE-SR4", "FAKE-VENDOR" in out and "FAKE-QSFP28-SR4" in out and "100GBASE-SR4" in out, out)
    wait("xcvrd: TRANSCEIVER_DOM_SENSOR Ethernet124",
         "redis-cli -s /var/run/redis/redis.sock -n 6 hget 'TRANSCEIVER_DOM_SENSOR|Ethernet124' temperature | grep -q 30.5", 150)
    out = cli("show interfaces transceiver eeprom --dom Ethernet32/1")
    print(out)
    check("eeprom --dom Ethernet32/1: temperature 30.5 C, threshold page", "30.5" in out and "75.0" in out, out)
    out = cli("show interfaces transceiver eeprom Ethernet1/1")
    print(out)
    check("eeprom Ethernet1/1: FAKE-SFP28-DAC", "FAKE-SFP28-DAC" in out, out)
    check("page 0 selected after the threshold reads",
          sh("od -An -tu1 -j127 -N1 " + ROOT + "/i2c/port32.bin")[1].strip() == "0")

    # hot-plug: CPLD presence (ModPrsL from the image files) -> get_change_event -> xcvrd
    sh("mv " + ROOT + "/i2c/port32.bin /tmp/port32.bin")
    wait("unplug Ethernet32/1: TRANSCEIVER_INFO removed",
         "test $(redis-cli -s /var/run/redis/redis.sock -n 6 exists 'TRANSCEIVER_INFO|Ethernet124') = 0", 60)
    out = cli("show interfaces transceiver presence Ethernet32/1")
    check("unplug: presence Not present", "Not present" in out, out)
    sh("mv /tmp/port32.bin " + ROOT + "/i2c/port32.bin")
    wait("replug Ethernet32/1: TRANSCEIVER_INFO back",
         "redis-cli -s /var/run/redis/redis.sock -n 6 hget 'TRANSCEIVER_INFO|Ethernet124' model | grep -q FAKE-QSFP28-SR4", 60)

    # --- fans, temperatures, PSUs, syseeprom
    wait("thermalctld: 12 FAN_INFO entries",
         "test $(redis-cli -s /var/run/redis/redis.sock -n 6 --scan --pattern 'FAN_INFO|*' | wc -l) = 12", 120)
    out = cli("show platform fan")
    print(out)
    check("show platform fan: 12 rotors OK, 59%", sum(1 for l in out.splitlines() if " OK " in l + " ") == 12 and "59%" in out, out)
    wait("thermalctld: 5 TEMPERATURE_INFO entries",
         "test $(redis-cli -s /var/run/redis/redis.sock -n 6 --scan --pattern 'TEMPERATURE_INFO|*' | wc -l) = 5", 120)
    out = cli("show platform temperature")
    print(out)
    check("show platform temperature: board_1 30.5, asic 41.5, thresholds 45/50",
          re.search(r"board_1\s+30.5\s+45\s+N/A\s+50", out) and re.search(r"asic\s+41.5\s+95\s+N/A\s+100", out), out)
    wait("psud: PSU_INFO", "redis-cli -s /var/run/redis/redis.sock -n 6 hget 'PSU_INFO|PSU 1' presence | grep -q true", 120)
    out = cli("show platform psustatus")
    print(out)
    check("show platform psustatus: PSU 1 OK, PSU 2 NOT PRESENT",
          any(l.startswith("PSU 1") and "OK" in l for l in out.splitlines())
          and any(l.startswith("PSU 2") and "NOT PRESENT" in l for l in out.splitlines()), out)
    wait("syseepromd: EEPROM_INFO", "redis-cli -s /var/run/redis/redis.sock -n 6 hget 'EEPROM_INFO|State' Initialized | grep -q 1", 120)
    out = cli("show platform syseeprom")
    print(out)
    check("show platform syseeprom: product, serial, MAC, CRC valid",
          "7760-32X-A-AC-F" in out and "776032X16521259" in out and "8C:EA:1B:89:C9:3C" in out and "(checksum valid)" in out, out)

    # --- fan control untouched by pmon
    check("pwm1 never written (fand owns it)",
          sh("stat -c '%Y %s' " + ROOT + "/sys/class/hwmon/hwmon0/pwm1; cat " + ROOT + "/sys/class/hwmon/hwmon0/pwm1")[1] == pwm_before)
    tl = sh("journalctl -b --no-pager -o cat -u sonic-thermalctld")[1]
    check("thermalctld: no thermal policy loaded", "policy" not in tl.lower() and "Thermal manager" not in tl, tl)

    # --- LLDP on the self-loop
    wait("lldpd: Ethernet16 sees Ethernet6/1 (port ID = alias, set by lldpmgrd)",
         "lldpctl Ethernet16 | grep -q 'PortID:.*local Ethernet6/1'", 240)
    wait("lldpd: Ethernet20 sees Ethernet5/1",
         "lldpctl Ethernet20 | grep -q 'PortID:.*local Ethernet5/1'", 120)
    out = cli("show lldp neighbors")
    print(out)
    check("show lldp neighbors: both directions, SysName switch",
          "Ethernet6/1" in out and "Ethernet5/1" in out and re.search(r"SysName:\s+switch", out), out)
    out = cli("show lldp neighbors Ethernet5/1")
    check("show lldp neighbors Ethernet5/1: only that port, peer Ethernet6/1",
          re.search(r"Interface:\s+Ethernet5/1,", out) and "local Ethernet6/1" in out and "local Ethernet5/1" not in out, out)
    out = cli("show lldp table")
    print(out)
    check("show lldp table: 2 entries", "Total entries displayed:  2" in out, out)
    wait("lldp_syncd: APPL_DB LLDP_ENTRY_TABLE:Ethernet16",
         "redis-cli -s /var/run/redis/redis.sock -n 0 hget LLDP_ENTRY_TABLE:Ethernet16 lldp_rem_port_id | grep -q Ethernet6/1", 120)
    check("CONFIG_DB FEATURE|lldp enabled (seed)", db(4, "hget 'FEATURE|lldp' state") == "enabled")

    rep = sh("for u in fake-as7760 sonic-xcvrd sonic-thermalctld sonic-psud sonic-syseepromd sonic-lldpd sonic-lldpmgrd sonic-lldp-syncd sonic-syncd sonic-orchagent; do echo $u $(systemctl show -p ActiveState -p NRestarts --value $u.service | tr '\\n' ' '); done")[1]
    print(rep)
    check("pmon/LLDP units active, no restarts",
          all(l.split()[1] == "active" and (len(l.split()) < 3 or l.split()[2] == "0") for l in rep.strip().splitlines()), rep)
    if FAILS:
        print(sh("journalctl -b --no-pager -o cat -u sonic-xcvrd -u sonic-thermalctld -u sonic-psud -u sonic-syseepromd -u sonic-lldpd -u sonic-lldpmgrd -u sonic-lldp-syncd | tail -200")[1])
    print("SUMMARY: %d failed: %s" % (len(FAILS), FAILS))
    assert not FAILS, FAILS
  '';
}
