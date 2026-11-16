# The monorepo's Rust NixOS modules (fand, consoled), found next to this tree:
#   rust = import ../rust-modules.nix;   # null when unavailable
# A git flake over the repository (our root flake, or github:nepeat/nepeat?dir=ai_re imported by
# another flake) contains rust/; `path:./nix` does not, and the result is null (the modules then
# assert). Not a module argument on purpose: a defaulted argument that isn't in specialArgs is
# looked up in config._module.args, and using it in `imports` is infinite recursion.
let
  root = ../../..;   # nix/modules -> nix -> ai_re -> the repository
  fand = root + "/rust/crates/fand/nix/module.nix";
  consoled = root + "/rust/consoled/nix/module.nix";
in
if builtins.pathExists fand && builtins.pathExists consoled then { nixosModules = { inherit fand consoled; }; }
else null
