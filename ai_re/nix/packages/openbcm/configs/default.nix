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
#
#   .tomahawkBase { globals = <reference config>; cores = [ { lane = 1; tx = "0x0123"; rx = "0x3210"; } ... ]; }
# A base for .tomahawk made of a board's own per-core values: the global keys
# of a reference config (its ports and every per-port key are dropped) plus one
# 100G port per listed core (lane = the core's first physical port) with these
# lane maps and, if given, txPolarity / rxPolarity (4-bit core masks). For a
# board whose ASIC lanes go somewhere else than the reference board's (e.g. to
# retimer PHYs): no pre-emphasis or driver current is carried over.
#
#   .ledProgram { src = <led_proc_init.soc>; label = "<where it is from>"; }
# An LED-processor rc script (SAI_OPENBCM_RC_FILE, rcload after "init all"),
# copied unchanged; the build fails if it holds anything but LEDUP port-order
# remaps, "led <n> prog|start|stop" and "led auto on|off" (comments allowed).
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

  # logical numbers and the keys that are dropped: tomahawk.py (logical_of, GENERATED)
  tomahawkBase = { name ? "tomahawk-base.config.bcm", globals, cores }:
    pkgs.runCommand name {
      nativeBuildInputs = [ pkgs.buildPackages.python3 ];
      inherit globals;
      passAsFile = [ "cores" ];
      cores = builtins.toJSON cores;
    } ''
      python3 -I ${./tomahawk.py} base --globals "$globals" --cores "$coresPath" --out $out
    '';

  ledProgram = { name ? "led_proc_init.soc", src, label ? "a reference LED program" }:
    pkgs.runCommand name { inherit src label; } ''
      bad=$(grep -nEv '^[[:space:]]*(#.*)?$|^m CMIC_LEDUP[01]_PORT_ORDER_REMAP_[0-9]+_[0-9]+([[:space:]]+REMAP_PORT_[0-9]+=[0-9]+)+[[:space:]]*$|^led [01] prog([[:space:]]+[0-9A-Fa-f]{2})+[[:space:]]*$|^led [01] (start|stop)[[:space:]]*$|^led auto (on|off)[[:space:]]*$' "$src" || true)
      if [ -n "$bad" ]; then echo "$label: unexpected commands in the LED program:"; echo "$bad"; exit 1; fi
      grep -q '^led auto on' "$src" || { echo "$label: no 'led auto on' (link state would not reach the LED processor)"; exit 1; }
      cp "$src" $out
    '';
}
