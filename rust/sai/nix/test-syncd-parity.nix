# Parity test (NixOS VM, no hardware): sai-openbcm's own smoke test, UNCHANGED
# (ai_re/nix/packages/sai-openbcm/tests/sai_smoke.py, passed in as `smoke`), through
# libsairedis -> redis -> syncd -> the Rust libsai on its virtual backend, then the same
# ASIC_DB expectations as the C++ test (ai_re/nix/packages/sai-openbcm/test-syncd.nix).
#
# This file is a labelled copy of that test-syncd.nix, not an import of it: the original
# takes `libsai` but hardcodes the C++ profile (SAI_OPENBCM_BACKEND=fake) and asserts two
# lines of the C++ library's log. The differences, all of them:
#   - profile: SAI_RS_BACKEND=virtual (the C++ one: SAI_OPENBCM_BACKEND=fake, SAI_OPENBCM_LOG_STDERR=1)
#   - log: "VIRTUAL backend selected" instead of "FAKE backend selected"; no "oper up" line
#     (the Rust library does not log link changes; the notification itself is checked, as there)
#   - two more expectations: 256 scheduler groups and the two hash objects, which the Rust
#     switch has and the C++ fake only has with its QoS / hash switches on
# With the two parameters named in ../README.md ("Parity") added to the original, this file
# becomes an import of it.
#   import ./test-syncd-parity.nix { pkgs = p; sonic = <ai_re sonic set>; libsai = <./package.nix>;
#                                    smoke = <ai_re>/nix/packages/sai-openbcm/tests/sai_smoke.py; }
{ pkgs
, sonic
, libsai
, smoke
, sairedis ? sonic.sairedis-openbcm libsai
}:
let
  py = pkgs.python3;
  site = "${py.sitePackages}";
  pythonPath = "${sairedis}/${site}/sairedis:${sonic.swss-common}/${site}";
  profile = pkgs.writeText "sai-rs-virtual.profile" ''
    SAI_RS_BACKEND=virtual
  '';
in
pkgs.testers.runNixOSTest {
  name = "sai-rs-syncd-parity";
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
    # Notifications syncd publishes (port state changes) land here.
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
    status, out = machine.execute("cd /tmp && PYTHONPATH=${pythonPath} timeout 300 python3 ${smoke} 2>&1")
    print("SMOKE exit", status)
    print(out)
    print("SYNCD LOG")
    print(machine.succeed("tail -c 6000 /tmp/syncd.txt"))
    print("SYSLOG (syncd): what it asked for and did not get")
    print(machine.succeed("journalctl -t syncd --no-pager | grep -v ' INFO \\| DEBUG ' | tail -n 150 | cut -c1-300"))
    print("ASIC_DB object counts")
    print(machine.succeed(
        "redis-cli -s /var/run/redis/redis.sock -n 1 --scan --pattern 'ASIC_STATE:*' | cut -d: -f2 | sort | uniq -c"))
    assert status == 0, out
    assert "SMOKE_OK" in out, out
    assert "port0_lanes 0 [1, 2, 3, 4]" in out, out
    assert "create_rif 0" in out, out
    assert "port0_oper_up_notified" in out, out

    def count(pattern):
        return int(machine.succeed(
            "redis-cli -s /var/run/redis/redis.sock -n 1 --scan --pattern '" + pattern + "' | wc -l").strip())

    expected = {
        "ASIC_STATE:SAI_OBJECT_TYPE_SWITCH:*": 1,
        "ASIC_STATE:SAI_OBJECT_TYPE_PORT:*": 33,                # 32 front panel + CPU
        "ASIC_STATE:SAI_OBJECT_TYPE_QUEUE:*": 256,              # 8 per front-panel port
        "ASIC_STATE:SAI_OBJECT_TYPE_INGRESS_PRIORITY_GROUP:*": 256,
        "ASIC_STATE:SAI_OBJECT_TYPE_VLAN:*": 2,                 # default 1 + 100
        "ASIC_STATE:SAI_OBJECT_TYPE_VLAN_MEMBER:*": 32,
        "ASIC_STATE:SAI_OBJECT_TYPE_BRIDGE:*": 1,
        "ASIC_STATE:SAI_OBJECT_TYPE_BRIDGE_PORT:*": 32,
        "ASIC_STATE:SAI_OBJECT_TYPE_VIRTUAL_ROUTER:*": 1,
        "ASIC_STATE:SAI_OBJECT_TYPE_STP:*": 1,
        "ASIC_STATE:SAI_OBJECT_TYPE_HOSTIF_TRAP_GROUP:*": 1,
        "ASIC_STATE:SAI_OBJECT_TYPE_ROUTER_INTERFACE:*": 1,
        # not in the C++ test (see the header)
        "ASIC_STATE:SAI_OBJECT_TYPE_SCHEDULER_GROUP:*": 256,
        "ASIC_STATE:SAI_OBJECT_TYPE_HASH:*": 2,
    }
    got = {k: count(k) for k in expected}
    print(got)
    for k, v in expected.items():
        assert got[k] == v, (k, got[k], v)

    vlan100 = machine.succeed(
        "redis-cli -s /var/run/redis/redis.sock -n 1 --scan --pattern 'ASIC_STATE:SAI_OBJECT_TYPE_VLAN:*'"
        " | while read k; do redis-cli -s /var/run/redis/redis.sock -n 1 hget \"$k\" SAI_VLAN_ATTR_VLAN_ID; done")
    assert "100" in vlan100.split(), vlan100
    assert int(machine.succeed("redis-cli -s /var/run/redis/redis.sock -n 1 hlen VIDTORID").strip()) > 600

    syncd_log = machine.succeed("cat /tmp/syncd.txt")
    assert "VIRTUAL backend selected" in syncd_log, syncd_log
    machine.succeed("grep -q port_state_change /tmp/notifications.txt")   # what syncd published
    print(machine.succeed("head -20 /tmp/notifications.txt"))
    machine.succeed("systemctl stop syncd")
  '';
}
