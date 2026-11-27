# syncd + the SONiC control plane on the Rust Teralynx libsai over the mock libifcs (NixOS VM, no chip):
# the CS8210's own board layer (ai_re/nix/modules/boards/cs8210/sonic.nix: port map, generated IFCS init
# yaml and its sidecars, sai.profile, cs8210-sai-files.service) with a stand-in vendor SDK tree whose
# libifcs.so is the C mock (rust/sai/tests/teralynx-mock, ./teralynx-mock.nix, as the parity harness has it), the Rust
# libsai-teralynx-shim.so next to the Rust libsai, and the SDK's inno_0_<sysport> netdevs (made by
# libifcs' ipd driver on the box) stood in for by dummies, so the default netdev mode (adopt) runs (a
# permanent neighbor on one more dummy stands in for the management port's: neighsyncd fills its link
# cache at its start, as on the box). The
# mock's call log is streamed to a file by the parity tap (LD_PRELOAD into syncd) and read by the test
# (./test-syncd-teralynx.py): create_switch, the 32 ports in ASIC_DB and their adopted netdevs, then
# orchagent-driven basics (ports admin up, link up through the mock's link-scan callback (a test hook
# preloaded into syncd), a VLAN with a member, a RIF + neighbor + route, an ACL table and rule, CoPP traps
# and policers), each checked in ASIC_DB and in the mock's IFCS calls.
# What the stand-ins do not show: anything of the real libifcs (its config parsing, link, netdevs, traps
# on the wire); the vendor tables' values (the stand-in tables have one synthetic line per cage).
#   import ./test-syncd-teralynx.nix { pkgs = p; sonic = <ai_re sonic set>; aiReNix = <ai_re/nix>;
#     saiRs = <./package.nix>; [sairedis = <syncd built against any libsai>]; }
# ai_re: target "vm-sai-teralynx-syncd" (just test-vm -k sai-teralynx).
{
  pkgs,
  sonic,
  aiReNix,
  saiRs,
  # the Rust libsai (sai-ffi, feature teralynx), the Rust shim, the mock + tap (parity fixtures)
  libsai ? pkgs.callPackage ./libsai-teralynx.nix { inherit saiRs; },
  shim ? pkgs.callPackage ./ifcs-shim.nix { inherit saiRs; },
  parity ? pkgs.callPackage ./teralynx-parity.nix { inherit saiRs; },
  # syncd: linked against libsai.so.1 of any build (the SAI C API); LD_LIBRARY_PATH (sai.libraryPath)
  # puts this test's library first
  sairedis ? sonic.sairedis-openbcm libsai,
}:
let
  fx = "${parity}/libexec/sai-teralynx-parity";
  # libsai with the shim next to it (where the rim's dladdr looks)
  libDir = pkgs.runCommand "sai-rs-teralynx-libdir" { } ''
    mkdir -p $out/lib
    cp ${libsai}/lib/libsai.so.1.0.0 $out/lib/
    ln -s libsai.so.1.0.0 $out/lib/libsai.so.1
    ln -s libsai.so.1 $out/lib/libsai.so
    cp ${shim}/lib/libsai-teralynx-shim.so $out/lib/
  '';
  # the vendor SDK tree's layout (hardware.cs8210.sonic.sdkDir), nothing vendor in it
  sdk = pkgs.runCommand "cs8210-sdk-standin" { } ''
    mkdir -p $out/cel_configs $out/cel_cmds
    cp ${fx}/mock/libifcs.so $out/libifcs.so
    printf '# stand-in isg.yaml (vm-sai-teralynx-syncd): no lane swaps\n' > $out/cel_configs/isg.yaml
    # one synthetic line per cage (host side 1-16, optics 17-32), tx_eq_pre1 = 1..8: marks in the call log
    for c in $(seq 1 16); do echo "ifcs set devport $c tx_eq_pre1 [1,2,3,4,5,6,7,8]"; done > $out/cel_cmds/preemphasis_PAM4_400G_32_HostLT.cmd
    for c in $(seq 17 32); do echo "ifcs set devport $c tx_eq_pre1 [1,2,3,4,5,6,7,8]"; done > $out/cel_cmds/preemphasis_PAM4_400G_32_Optics.cmd
  '';
  calls = "/var/lib/sonic-syncd/ifcs-calls.txt";
  # link changes for the mock (it has no link of its own: every devport stays down): preloaded into
  # syncd, a thread reads "<devport> <0|1>" lines from linkFile once a second and calls the mock's
  # test hook mock_fire_link (its link scan callback into the SAI, as test_ifcs does), then removes it
  linkFile = "/run/teralynx-mock-link";
  mockLink = pkgs.runCommandCC "teralynx-mock-link" { } ''
    mkdir -p $out/lib
    cat > link.c << 'EOF'
    #define _GNU_SOURCE
    #include <dlfcn.h>
    #include <pthread.h>
    #include <stdio.h>
    #include <unistd.h>
    static void* run(void* arg)
    {
        (void)arg;
        for (;;) {
            sleep(1);
            FILE* f = fopen("${linkFile}", "r");
            if (!f) continue;
            void* h = dlopen("${sdk}/libifcs.so", RTLD_NOW | RTLD_NOLOAD);
            int (*fire)(unsigned, int) = h ? (int (*)(unsigned, int))dlsym(h, "mock_fire_link") : NULL;
            unsigned dp; int up;
            while (fire && fscanf(f, "%u %d", &dp, &up) == 2)
                fprintf(stderr, "teralynx-mock-link: devport %u %s: %d\n", dp, up ? "up" : "down", fire(dp, up));
            fclose(f);
            if (fire) unlink("${linkFile}");
            if (h) dlclose(h);
        }
        return NULL;
    }
    __attribute__((constructor)) static void start(void)
    {
        pthread_t t;
        if (pthread_create(&t, NULL, run, NULL) == 0) pthread_detach(t);
    }
    EOF
    $CC -std=gnu11 -O2 -Wall -Wextra -Werror -shared -fPIC -o $out/lib/libteralynx-mock-link.so link.c -ldl -lpthread
  '';
in
pkgs.testers.runNixOSTest {
  name = "sai-rs-teralynx-syncd";
  nodes.switch = { lib, options, ... }: {
    imports = [ (aiReNix + "/modules/switch/sonic") (aiReNix + "/modules/boards/cs8210/sonic.nix") ];
    virtualisation.memorySize = 3072;
    virtualisation.cores = 2;
    networking.firewall.enable = false;
    environment.systemPackages = [ sonic.swss pkgs.redis ];   # swssconfig: an APPL_DB route without FRR
    hardware.cs8210.sonic.sdkDir = "${sdk}";
    services.sonic = {
      enable = true;
      packages = sonic;
      macAddress = "52:54:00:aa:bb:01";
      sai = {
        kind = "vendor";
        package = sairedis;
        libraryPath = [ "${libDir}/lib" ];
        # as the box (fand's input): the frontend writes the mock's temperatures there
        profile.SAI_OPENBCM_TEMP_FILE = "/run/sai-teralynx/asic-temperature";
      };
      kernelModules = [ "team" "team_mode_loadbalance" "bridge" "8021q" "dummy" ];
      settings.PORT = lib.genAttrs [ "Ethernet1/1" "Ethernet2/1" "Ethernet3/1" ] (_: { admin_status = "up"; });
    } // lib.optionalAttrs (options.services.sonic ? frr) {
      frr.enable = false;   # the route comes from APPL_DB (swssconfig), not FRR
    };
    # what libifcs' ipd driver makes on the box (init yaml netdev auto_create): inno_0_<sysport>
    systemd.services.teralynx-standin-netdevs = {
      description = "Stand-ins for the SDK's inno_0_<sysport> netdevs (adopted by the SAI)";
      before = [ "sonic-syncd.service" ];
      requiredBy = [ "sonic-syncd.service" ];
      path = [ pkgs.iproute2 ];
      serviceConfig = { Type = "oneshot"; RemainAfterExit = true; };
      script = ''
        for i in $(seq 1 32); do
          n=$(printf 'inno_0_%04d' $i)
          ip link show $n >/dev/null 2>&1 || ip link show $(printf 'Ethernet%d' $(( (i - 1) * 8 ))) >/dev/null 2>&1 || { ip link add $n type dummy && ip link set $n arp on; }   # ARP: neighbors as on an ipd netdev
        done
      '';
    };
    # the box's management port always has neighbors when neighsyncd starts, so neighsyncd fills its
    # ifindex -> name link cache at once; with the SDK's netdevs already there (here from boot), a cache
    # filled before their renames to Ethernet* kept the inno_0_* names: services.sonic.sai.renamesNetdevs
    networking.dhcpcd.denyInterfaces = [ "mgmt-standin" ];   # a down/up by dhcpcd flushed the neighbor
    systemd.services.mgmt-neighbor-standin = {
      description = "A management-port neighbor in neighsyncd's first dump";
      before = [ "sonic-syncd.service" ];
      requiredBy = [ "sonic-syncd.service" ];
      path = [ pkgs.iproute2 ];
      serviceConfig = { Type = "oneshot"; RemainAfterExit = true; };
      script = ''
        # a MAC of its own: udev's MACAddressPolicy=persistent changed a random one later, flushing the neighbor
        ip link show mgmt-standin >/dev/null 2>&1 || ip link add mgmt-standin address 02:00:00:00:00:fe type dummy
        ip link set mgmt-standin arp on up
        ip neigh replace 192.0.2.1 lladdr 02:00:00:00:00:01 dev mgmt-standin nud permanent
      '';
    };
    # the mock's call log, every line (the mock keeps it only in memory)
    systemd.services.sonic-syncd.environment = {
      LD_PRELOAD = "${fx}/bin/libparity-tap.so ${mockLink}/lib/libteralynx-mock-link.so";
      PARITY_TAP_FILE = calls;
      PARITY_MOCK = "${sdk}/libifcs.so";
    };
  };
  testScript = ''
    CALLS = "${calls}"
    LINK = "${linkFile}"
    SDK = "${sdk}"
    LIBDIR = "${libDir}/lib"
  '' + builtins.readFile ./test-syncd-teralynx.py;
}
