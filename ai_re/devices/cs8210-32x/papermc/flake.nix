{
  description = "CS8210 PaperMC server (4 GiB service budget)";
  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
  outputs = { self, nixpkgs }:
    let
      systems = [ "aarch64-linux" "x86_64-linux" ];
      each = nixpkgs.lib.genAttrs systems;
    in {
      packages = each (system:
        let pkgs = import nixpkgs { inherit system; };
        in {
          default = pkgs.writeShellScriptBin "papermc" ''
            export JAVA_TOOL_OPTIONS="-Xms1G -Xmx2560M -XX:+UseG1GC -XX:ActiveProcessorCount=4"
            exec ${pkgs.papermc}/bin/minecraft-server "$@"
          '';
          server = pkgs.papermc;
          just = pkgs.just;
          mcrcon = pkgs.mcrcon;
          worldedit = pkgs.fetchurl {
            url = "https://cdn.modrinth.com/data/1u6JkXh5/versions/YFnqiUlL/worldedit-bukkit-7.4.6.jar";
            hash = "sha256-4M5n2HCtH++KQN2deSY2DK87tEciQ4VF1rILVfgqDJE=";
          };
        });
      devShells = each (system:
        let pkgs = import nixpkgs { inherit system; };
        in {
          default = pkgs.mkShellNoCC {
            packages = [ pkgs.just pkgs.python3 pkgs.mcrcon ];
          };
        });
      apps = each (system: {
        default = {
          type = "app";
          program = "${self.packages.${system}.default}/bin/papermc";
        };
      });
    };
}
