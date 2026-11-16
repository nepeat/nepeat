# NixOS modules exported by ai_re (flake: nixosModules.<name>; plain Nix:
# (import ./ai_re/nix).nixosModules.<name>). Layers, most to least reusable:
#   profiles/   policy with no hardware knowledge
#   platforms/  one SoC family (kernel, boot image format, console)
#   thermal/, monitoring/ ...  one function, shared by boards using the same part
#   boards/     one board = platform + functions + board deltas
{
  profile-appliance = ./profiles/appliance.nix;
  profile-rebuildable = ./profiles/rebuildable.nix;
  profile-systemd-minimal = ./profiles/systemd-minimal.nix;
  profile-netboot-root = ./profiles/netboot-root.nix;
  platform-qoriq-t2080 = ./platforms/qoriq-t2080.nix;
  thermal-as7760-fan = ./thermal/as7760-fan.nix;
  thermal-dmc12-fan = ./thermal/dmc12-fan.nix;
  switch-openbcm = ./switch/openbcm.nix;
  switch-sonic = ./switch/sonic;
  monitoring-node-exporter = ./monitoring/node-exporter.nix;
  board-as7760 = ./boards/as7760;
  board-as7760-sd-install = ./boards/as7760/sd-install.nix;
  board-as7760-sd-persist = ./boards/as7760/sd-persist.nix;
  # Verbatim import of codeberg dma/dmc12-nixos; see boards/dmc12/README.md.
  board-dmc12 = ./boards/dmc12;
}
