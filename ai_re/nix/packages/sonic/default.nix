# SONiC's switch-state stack as plain Nix packages (plan and status:
# ai_re/devices/sonic-nixos/PROGRESS.md). Upstream is the source of truth;
# sources.json is the ONLY place a SONiC revision appears (submodule SHAs of
# the pinned sonic-buildimage tree).
#   import ./sonic { inherit pkgs; }   # pkgs may be a cross package set
{ pkgs }:
let
  inherit (pkgs) lib;
  pins = lib.importJSON ./sources.json;
  fetch = name: pkgs.fetchFromGitHub { inherit (pins.${name}) owner repo rev hash; };
  self = {
    sources = lib.mapAttrs (name: _: fetch name) pins;
    version = name: "0-${lib.substring 0 7 pins.${name}.rev}";

    # SONiC patches libnl 3.7 for RTA_NH_ID (src/libnl3/patch/0003). nixpkgs'
    # libnl (>= 3.10) has that upstream, spelled rtnl_route_{get,set}_nhid;
    # consumers alias the SONiC spelling instead of us forking libnl.
    libnl-sonic = pkgs.libnl;
    nhIdAlias = "-Drtnl_route_get_nh_id=rtnl_route_get_nhid -Drtnl_route_set_nh_id=rtnl_route_set_nhid";
    swss-common = pkgs.callPackage ./swss-common { sonic = self; };
  };
in self
