{
  description = "lix + nixos + nepeat";

    inputs = {
        nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

        lix = {
            url = "git+https://git@git.lix.systems/lix-project/lix";
        };

        lix-module = {
            url = "git+https://git.lix.systems/lix-project/nixos-module";
            inputs.lix.follows = "lix";
            inputs.nixpkgs.follows = "nixpkgs";
        };
    };

    outputs = {
        self,
        lix-module,
        nixpkgs,
        ...
    }: {
    nixosConfigurations = {
            iso = nixpkgs.lib.nixosSystem {
                system = "x86_64-linux";
                modules = [
                    lix-module.nixosModules.default
                    ({ pkgs, modulesPath, ... }: {
                    imports = [ (modulesPath + "/installer/cd-dvd/installation-cd-minimal.nix") ];

                    services.openssh.enable = true;
                    users.users.root.openssh.authorizedKeys.keys = [
                        "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIMVk9i7FG7dc9r4ixwAJT7uPLH3UuqbwIgeZ7Ytmnpvv erin-laptop"
                        ];
                    })
                ];
            };
        };
    };
}
