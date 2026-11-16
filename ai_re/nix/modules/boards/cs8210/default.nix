# CS8210-32X ("Briggs") SONiC layer, imported by the machine (the board's
# NixOS config lives in the nepeat/nix flake): ./sonic.nix (ports, IFCS
# config, SAI profile, vendor files at runtime) and ./sonic-platform.nix
# (pmon, LLDP). Nothing is set until services.sonic.enable.
{
  imports = [ ./sonic.nix ./sonic-platform.nix ];
}
