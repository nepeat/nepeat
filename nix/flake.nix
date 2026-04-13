{
  description = "erin's base config";

  inputs = {
    # Package sets
    nixpkgs.url = "github:NixOS/nixpkgs/master";

    # lix
    lix = {
      url = "git+https://git@git.lix.systems/lix-project/lix";
    };

    lix-module = {
      url = "git+https://git.lix.systems/lix-project/nixos-module";
      inputs.lix.follows = "lix";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    # Environment/system management
    darwin.url = "github:lnl7/nix-darwin/master";
    darwin.inputs.nixpkgs.follows = "nixpkgs";
    home-manager.url = "github:nix-community/home-manager/master";
    home-manager.inputs.nixpkgs.follows = "nixpkgs";
    nix-rosetta-builder = {
      url = "github:cpick/nix-rosetta-builder";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    talhelper.url = "github:budimanjojo/talhelper";
    claude-code.url = "github:sadjow/claude-code-nix";

    nix-index-database.url = "github:nix-community/nix-index-database";
    nix-index-database.inputs.nixpkgs.follows = "nixpkgs";

  };

  outputs = {
    self,
    darwin,
    nixpkgs,
    lix-module,
    home-manager,
    nix-rosetta-builder,
    claude-code,
    ...
  } @inputs:
  let

    inherit (darwin.lib) darwinSystem;
    inherit (inputs.nixpkgs.lib) attrValues makeOverridable optionalAttrs singleton;

    # Configuration for `nixpkgs`
    nixpkgsConfig = {
      config = {
        allowUnfree = true;
      };
      overlays = attrValues self.overlays;
      # hostPlatform = "aarch64-darwin";
    };
  in
  {
    # home-manager
    homeConfigurations = rec {
      "erin" = home-manager.lib.homeManagerConfiguration {
        pkgs = import nixpkgs {
          system = "x86_64-linux";
          config.allowUnfree = true;
        };
        modules = [
          inputs.nix-index-database.homeModules.nix-index
          ./base/configuration.nix
          ./base/home.nix
          ./base/non_nix_home.nix
          ./machine/nonwork_home.nix
          {
            programs.home-manager.enable = true;
            home.homeDirectory = "/home/erin";
            home.username = "erin";
          }
        ];
      };
    };

    # nix-darwin
    darwinConfigurations = rec {
      newPersonal = darwinSystem {
        specialArgs = { inherit inputs; };
        modules = [
          lix-module.darwinModules.default
          # Main `nix-darwin` config
          ./base/configuration.nix
          ./base/system-packages.nix
          ./base/darwin.nix
          ./machine/m4mac.nix
          ./module/claude.nix
          inputs.nix-index-database.darwinModules.nix-index
          # `home-manager` module
          home-manager.darwinModules.home-manager
          {
            nixpkgs = nixpkgsConfig;
            # `home-manager` config
            home-manager.useGlobalPkgs = true;
            home-manager.useUserPackages = true;
            home-manager.users.nepeat = import ./base/home.nix;
          }
        ];
      };
      personal = darwinSystem {
        specialArgs = { inherit inputs; };
        modules = [
          lix-module.darwinModules.default
          # Main `nix-darwin` config
          ./base/configuration.nix
          ./base/system-packages.nix
          ./base/darwin.nix
          ./machine/m1laptop.nix
          ./module/claude.nix
          inputs.nix-index-database.darwinModules.nix-index
          # `home-manager` module
          home-manager.darwinModules.home-manager
          {
            nixpkgs = nixpkgsConfig;
            # `home-manager` config
            home-manager.useGlobalPkgs = true;
            home-manager.useUserPackages = true;
            home-manager.users.nep = import ./base/home.nix;
            environment.etc."machine_name".text = "personal";
          }
        ];
      };
    };

    # Overlays --------------------------------------------------------------- {{{

    overlays = {
        # Overlay useful on Macs with Apple Silicon
        apple-silicon = final: prev: optionalAttrs (prev.stdenv.hostPlatform.system == "aarch64-darwin") {
          # Add access to x86 packages system is running Apple Silicon
          pkgs-x86 = import inputs.nixpkgs {
            system = "x86_64-darwin";
            inherit (nixpkgsConfig) config;
          };
        };
      };
 };
}
