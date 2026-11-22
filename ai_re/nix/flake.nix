{
  description = "ai_re shared Nix: packages, NixOS modules and system configs for reverse-engineered hardware";
  # Pinned separately from the ai_re devshell: these systems track nixos-unstable.
  # Keep the root flake's nixpkgs-systems on the same revision ("just update" does; a test checks).
  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  # No fand here: a path flake cannot reach ../../rust. The ai_re root flake
  # (a git flake over the whole monorepo) passes it; see ../flake.nix.
  outputs = { self, nixpkgs }: (import ./.).flakeOutputs { inherit nixpkgs; };
}
