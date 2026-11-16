# CS8210-32X IFCS init config (config.yaml) for services.sonic.sai.sdkConfig:
# the devports of the SONiC ports (packages/sai-teralynx/configs, rules and
# vendor sources there) with the board values of the vendor's knet bring-up
# (Briggs SDK V0.1.7: auto_load_user.sh -k, cel_configs/common_PAM4_400G_32_nd.yaml
# sha256 66fc02f8...; dynamic_config.sh 805eef45...). Checked against that
# dynamic_config.sh by devices/cs8210-32x/scripts/sonic_sdk_config_check.py
# (evidence: devices/cs8210-32x/sonic-sdk-config-20261115.txt).
#   import ./sdk-config.nix { inherit pkgs sonic; media = { "17" = "copper"; }; } { ports = [ ... ]; settings = { }; }
# Board values:
#   sku          the public sonic-buildimage Midstone-200i_32x400 innovium.77700_A
#                (byte-identical to the SDK's configs/sku/innovium.77701_A).
#   node         as the vendor's _nd file. Clocks, rings and queues equal the
#                public Midstone-200i_32x400 yaml (config_32x400G_midstone200i.yaml);
#                it differs in: netdev auto_create "no" + multi_interface "yes"
#                (SAI-created netdevs; here "yes", the SDK makes one per devport),
#                txring 0 netdev "true", buffer_management_mode "api_driven",
#                max_lossless_tc "2", and no skip_pll_check/mbist_on_init.
#                Switch with settings, e.g. "netdev.auto_create" = "no".
#   cages 1-16   side "host" (ASIC lanes to the Credo PHYs): FEC only in the
#                PAM4 modes 1x400/4x100/8x50 (KP), ipg 48, media copper (host
#                link training), as cs8210-switch-up's default spec.
#   cages 17-32  side "line": the vendor's fec-def per media; media optics unless
#                media."<cage>" says copper (DAC: link training, other taps).
#   sFlow        traffic_monitor_cpu_queue "2": sFlow samples on CPU queue 2,
#                SONiC's sflow trap group's queue (boot-time: the node attribute
#                is refused at run time).
#   devport 0    cpu, sysport 1000; devport 129 the 10G AUX lane (serdes group
#                32 lane 0) to the CPU card's second NIC, type "mgmt 0", fec NONE.
# Pre-emphasis and the isg (lane swap/polarity) section are vendor tuning data
# and come from the SDK tree at runtime (./sai-files.py), not from here.
{ pkgs, sonic, media ? { }, teralynx ? import ../../../packages/sai-teralynx/configs { inherit pkgs; } }:
{ ports, settings ? { } }:
let
  inherit (pkgs) lib;
  cages = import ../../switch/sonic/port-maps/celestica-cs8210-32x.nix;
  skuPath = "device/celestica/x86_64-cel_midstone-r0/Midstone-200i_32x400/innovium.77700_A";
  # Only this file goes to the switch, not the whole source tree.
  sku = pkgs.runCommand "innovium.77700_A" { } "cp ${sonic.sources.buildimage}/${skuPath} $out";
  ring = kind: id: extra: { "${kind}ring_id" = toString id; desc_count = "1024"; prio = "1"; } // extra;
  queues = r: lib.concatMapStringsSep ", " (i: toString (r + 4 * i)) (lib.range 0 11);
in
teralynx.teralynx7 {
  name = "cs8210-32x-config.yaml";
  label = "CS8210-32X: Briggs SDK V0.1.7 knet node options, sonic-buildimage ${builtins.substring 0 12 sonic.sources.buildimage.rev} ${skuPath}";
  node = {
    sku = "${sku}";
    netdev = [ { auto_create = "yes"; } ];
    mac_clk = "1340";
    skip_pll_check = "false";
    sys_clk = "1720";
    mbist_on_init = "true";
    ifc_clk = "1200";
    traffic_monitor_cpu_queue = "2";
    txring = [ (ring "tx" 0 { }) ] ++ map (i: ring "tx" i { netdev = "true"; }) [ 1 2 3 ];
    rxring = map (i: ring "rx" i { queues = queues i; netdev = "true"; }) [ 0 1 2 3 ];
  };
  inherit settings cages ports;
  cageOptions = lib.imap1 (i: _: {
    side = if i <= 16 then "host" else "line";
    media = media.${toString i} or (if i <= 16 then "copper" else "optics");
  }) cages;
  hostIpg = "48";
  cpu = { id = 0; sysport = 1000; };
  extraDevports = [ { id = 129; lanes = "0:1"; serdes_group = 32; sysport = 129; type = "mgmt 0"; fec = "NONE"; } ];
}
