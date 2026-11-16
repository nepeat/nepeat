# Teralynx 7 (IVM77700) IFCS init config (config.yaml) from front-panel ports.
# Build-host only: the result is a text file (SAI_INIT_CONFIG_FILE).
#   (import ai_re/nix/packages/sai-teralynx/configs { inherit pkgs; }).teralynx7 {
#     name = "config.yaml"; label = "<where the board values are from>";
#     node = { sku = <file>; sys_clk = "1720"; txring = [ ... ]; ... };  # node options
#     settings = { key = "value"; other = null; };   # node options; null removes;
#                                                    # "netdev.<k>", "log_level"
#     cages = [ [ <lanes> ] ... ];                    # services.sonic.cages
#     cageOptions = [ { side = "host"|"line"; media = "copper"|"optics"; } ... ];
#     hostIpg = "48";                                 # ipg of host-side devports (null: none)
#     cpu = { id = 0; sysport = 1000; };
#     extraDevports = [ { id = 129; type = "mgmt 0"; ... } ];   # written as given
#     ports = [ { name; alias; lanes; speed; } ... ]; # SONiC PORT entries
#   }
# Rules (lane numbering, devport ids, port modes per serdes group, FEC per mode
# and media, link training) and their vendor sources: teralynx7.py. The build
# fails unless the written config, read back, has exactly these ports (lanes,
# speeds) as eth devports, numbered as planned, and the cpu/extra devports.
# The board's isg section (lane swap, polarity) is not generated.
#
# Outputs: out = the yaml; sidecars = a directory with
#   port-map           SAI_TERALYNX_PORT_MAP: "devport lanes speed_mbps fec" per eth devport
#   init.cmd           "ifcs set devport <id> rx_eq_fine_tune 1" per eth devport + extra devports
#   link-training.cmd  "ifcs set devport <id> link_training 1" (copper, PAM4 1x400/4x100/8x50)
#   layout.json        per eth devport: cage, subport, mode, side, media, serdes group, fec
# (also as passthru paths: portMap, initCmds, linkTraining, layout).
{ pkgs }:
{
  # Speeds by lane count a devport can run (services.sonic.validSpeeds).
  # Which combinations a cage (serdes group) takes: GROUP_MODES in teralynx7.py.
  validSpeeds = {
    "1" = [ 10000 25000 50000 ];
    "2" = [ 100000 ];
    "4" = [ 40000 100000 200000 ];
    "8" = [ 200000 400000 ];
  };

  teralynx7 = { name ? "config.yaml", label ? "", node, settings ? { }, logLevel ? "info",
                cages, cageOptions, hostIpg ? null, cpu ? { id = 0; sysport = 1000; },
                extraDevports ? [ ], ports }:
    let
      drv = pkgs.runCommand name {
        outputs = [ "out" "sidecars" ];
        nativeBuildInputs = [ pkgs.buildPackages.python3 ];
        passAsFile = [ "spec" ];
        spec = builtins.toJSON {
          inherit label node settings logLevel cages cageOptions hostIpg cpu extraDevports;
          ports = map (p: { inherit (p) name alias lanes speed; }) ports;
        };
      } ''
        python3 -I ${./teralynx7.py} generate --spec "$specPath" --out $out --sidecars $sidecars
        python3 -I ${./teralynx7.py} check --spec "$specPath" $out
      '';
    in drv // {
      portMap = "${drv.sidecars}/port-map";
      initCmds = "${drv.sidecars}/init.cmd";
      linkTraining = "${drv.sidecars}/link-training.cmd";
      layout = "${drv.sidecars}/layout.json";
    };
}
