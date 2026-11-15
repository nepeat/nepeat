# Broadcom SDK config files (config.bcm) generated from front-panel ports.
# Build-host only: the result is a text file (SAI_INIT_CONFIG_FILE, bcm.user).
#   (import ai_re/nix/packages/openbcm { inherit pkgs; }).configs.tomahawk {
#     base = <reference config, one 100G port per core>;  label = "<where it is from>";
#     ports = [ { name; alias; lanes; speed; } ... ];      # SONiC PORT entries
#     management = [ { logical = 66; phys = 129; speed = 10; } ];
#     settings = { key = "value"; other = null; };        # global keys; null removes
#     laneKeys = [ "preemphasis" "driver_current" ];      # per-lane TX keys of base to keep
#   }
# The Tomahawk rules (logical numbering, lanes per speed, per-lane keys of a
# split core) and their SDK sources are in tomahawk.py. The build fails unless
# the written config, read back with the SDK's rules, has exactly these ports
# (lanes, speeds) and the base's lane maps, polarity and pre-emphasis.
{ pkgs }:
{
  tomahawk = { name ? "config.bcm", base, label ? "a reference config", ports, management ? [ ],
               settings ? { }, dportMap ? true, laneKeys ? [ "preemphasis" "driver_current" ] }:
    pkgs.runCommand name {
      nativeBuildInputs = [ pkgs.buildPackages.python3 ];
      inherit base;
      passAsFile = [ "spec" ];
      spec = builtins.toJSON {
        ports = map (p: { inherit (p) name alias lanes speed; }) ports;
        inherit management settings dportMap label laneKeys;
      };
    } ''
      python3 -I ${./tomahawk.py} generate --base "$base" --spec "$specPath" --out $out
      python3 -I ${./tomahawk.py} check --base "$base" --spec "$specPath" $out
    '';
}
