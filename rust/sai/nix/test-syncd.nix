# Integration test (NixOS VM, no hardware): SONiC's syncd linked against the Rust libsai
# (virtual backend), redis on the SONiC socket, sai_slice_smoke.py through libsairedis, then
# what syncd discovered in ASIC_DB. Modelled on the deleted C++ sai-openbcm's test-syncd.nix (./test-syncd-parity.nix).
# The caller passes the SONiC package set (this tree does not know where it lives):
#   import ./test-syncd.nix { pkgs = p; sonic = <ai_re sonic set>; libsai = <./package.nix>; }
# ai_re: target "vm-sai-rs-syncd" (just test-vm -k sai-rs). What it shows: ../README.md, "syncd".
# The profile uses the Rust alias (SAI_RS_BACKEND=virtual); vm-sai-rs-syncd-parity the C++ key.
{ pkgs
, sonic
, libsai
, sairedis ? sonic.sairedis-openbcm libsai
}:
let
  py = pkgs.python3;
  site = "${py.sitePackages}";
  pythonPath = "${sairedis}/${site}/sairedis:${sonic.swss-common}/${site}";
  profile = pkgs.writeText "sai-rs-virtual.profile" ''
    SAI_RS_BACKEND=virtual
    SAI_OPENBCM_LOG_STDERR=1
  '';
  smoke = ./sai_slice_smoke.py;
in
pkgs.testers.runNixOSTest {
  name = "sai-rs-syncd-virtual";
  nodes.machine = { ... }: {
    virtualisation.memorySize = 2048;
    environment.systemPackages = [ pkgs.redis py sairedis ];
  };
  testScript = ''
    machine.wait_for_unit("multi-user.target")
    machine.succeed(
        "mkdir -p /var/run/redis/sonic-db",
        "cp ${sonic.swss-common}/share/swsscommon/sonic-db/database_config.json /var/run/redis/sonic-db/",
        "redis-server --daemonize yes --port 6379 --unixsocket /var/run/redis/redis.sock"
        " --unixsocketperm 777 --save \"\" --appendonly no --databases 16 --logfile /tmp/redis.txt",
    )
    machine.wait_until_succeeds("redis-cli -s /var/run/redis/redis.sock ping | grep PONG")
    machine.succeed(
        "systemd-run --unit notif -p StandardOutput=file:/tmp/notifications.txt"
        " ${pkgs.coreutils}/bin/stdbuf -oL ${pkgs.redis}/bin/redis-cli -s /var/run/redis/redis.sock -n 1 subscribe NOTIFICATIONS"
    )
    machine.succeed(
        "systemd-run --unit syncd -p WorkingDirectory=/tmp -p StandardOutput=file:/tmp/syncd.txt"
        " -p StandardError=file:/tmp/syncd.txt -E LD_LIBRARY_PATH=${libsai}/lib"
        " ${sairedis}/bin/syncd -SUu -z redis_sync -p ${profile}"
    )
    machine.sleep(2)
    status, out = machine.execute("cd /tmp && PYTHONPATH=${pythonPath} timeout 120 python3 ${smoke} 2>&1")
    print("SMOKE exit", status)
    print(out)
    print("SYNCD LOG")
    print(machine.succeed("cat /tmp/syncd.txt | tail -c 6000"))
    print("SYSLOG (syncd)")
    print(machine.succeed("journalctl -t syncd --no-pager | tail -n 120 | cut -c1-400"))
    print("ASIC_DB object counts")
    print(machine.succeed(
        "redis-cli -s /var/run/redis/redis.sock -n 1 --scan --pattern 'ASIC_STATE:*' | cut -d: -f2 | sort | uniq -c"))
    def count(pattern):
        return int(machine.succeed(
            "redis-cli -s /var/run/redis/redis.sock -n 1 --scan --pattern '" + pattern + "' | wc -l").strip())

    # What syncd found by walking the switch after create_switch, plus VLAN 100.
    expected = {
        "ASIC_STATE:SAI_OBJECT_TYPE_SWITCH:*": 1,
        "ASIC_STATE:SAI_OBJECT_TYPE_PORT:*": 33,          # 32 front panel + CPU
        "ASIC_STATE:SAI_OBJECT_TYPE_VLAN:*": 2,           # default 1 + 100
        "ASIC_STATE:SAI_OBJECT_TYPE_VLAN_MEMBER:*": 32,
        "ASIC_STATE:SAI_OBJECT_TYPE_BRIDGE:*": 1,
        "ASIC_STATE:SAI_OBJECT_TYPE_BRIDGE_PORT:*": 32,
    }
    got = {k: count(k) for k in expected}
    assert got == expected, got
    assert "SMOKE_OK" in out, out
    assert "FAKE backend selected" in machine.succeed("cat /tmp/syncd.txt")   # the C++ line
    assert "port0_oper_up_notified" in out, out
    machine.succeed("grep -q port_state_change /tmp/notifications.txt")
    machine.succeed("systemctl stop syncd")
  '';
}
