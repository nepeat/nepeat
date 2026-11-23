# The units VM test of openbcm-mdio-script (ai_re/nix/packages/sai-openbcm/test-mdio-units.nix:
# hard fail, status files, systemd status text, login message, phy-status, the way back) with
# the Rust tool in place of the C tool. Not a copy: the original test is imported as it is and
# given, through its own `libsai` argument (the package whose bin/openbcm-mdio-script the test's
# stand-in wraps), a package with the Rust binary under that name. Everything else (module,
# stand-ins, test script) is the original's.
#   nix build --impure --expr 'let f = builtins.getFlake "path:<ai_re>/nix";
#     p = import f.inputs.nixpkgs { system = "x86_64-linux"; };
#     in import <monorepo>/rust/sai/nix/test-mdio-units.nix { pkgs = p; aiReNix = f.outPath; }'
{
  pkgs,
  # ai_re/nix: the flake's source (f.outPath) or the working tree
  aiReNix ? ../../../ai_re/nix,
  sonic ? import (aiReNix + "/packages/sonic") { inherit pkgs; },
  mdioTool ? pkgs.callPackage ./mdio-tool.nix { saiOpenbcmSrc = aiReNix + "/packages/sai-openbcm/src"; },
}:
import (aiReNix + "/packages/sai-openbcm/test-mdio-units.nix") {
  inherit pkgs sonic;
  libsai = mdioTool.asOpenbcmMdioScript;
}
