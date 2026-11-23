# The units VM test of the boot-time MDIO unit (ai_re/nix/modules/switch/tests/openbcm-mdio-units.nix:
# hard fail, status files, systemd status text, login message, phy-status, the way back) with the
# Rust tool, the one the switches run (until 2026-11-23 the test also ran on the C tool). The test is
# imported as it is and given, through its `libsai` argument (the package whose
# bin/openbcm-mdio-script the test's stand-in wraps), a package with the Rust binary under that name.
#   nix build --impure --expr 'let f = builtins.getFlake "path:<ai_re>/nix";
#     p = import f.inputs.nixpkgs { system = "x86_64-linux"; };
#     in import <monorepo>/rust/sai/nix/test-mdio-units.nix { pkgs = p; aiReNix = f.outPath; }'
{
  pkgs,
  # ai_re/nix: the flake's source (f.outPath) or the working tree
  aiReNix ? ../../../ai_re/nix,
  mdioTool ? pkgs.callPackage ./mdio-tool.nix { glueSrc = aiReNix + "/packages/sai-openbcm/src"; },
}:
import (aiReNix + "/modules/switch/tests/openbcm-mdio-units.nix") {
  inherit pkgs;
  libsai = mdioTool.asOpenbcmMdioScript;
}
