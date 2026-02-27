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
      hostPlatform = "aarch64-darwin";
    };
  in
  {
    # My `nix-darwin` configs

    darwinConfigurations = rec {
      newPersonal = darwinSystem {
        modules = [
          lix-module.darwinModules.default
          # Main `nix-darwin` config
          ./base/configuration.nix
          ./base/system-packages.nix
          ./base/darwin.nix
          ./machine/m4mac.nix
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
          # An existing Linux builder is needed to initially bootstrap `nix-rosetta-builder`.
          # If one isn't already available: comment out the `nix-rosetta-builder` module below,
          # uncomment this `linux-builder` module, and run `darwin-rebuild switch`:
          # { nix.linux-builder.enable = true; }
          # Then: uncomment `nix-rosetta-builder`, remove `linux-builder`, and `darwin-rebuild switch`
          # a second time. Subsequently, `nix-rosetta-builder` can rebuild itself.
          nix-rosetta-builder.darwinModules.default
          {
            # see available options in module.nix's `options.nix-rosetta-builder`
            nix-rosetta-builder.onDemand = true;
          }
        ];
      };
      personal = darwinSystem {
        modules = [
          lix-module.darwinModules.default
          # Main `nix-darwin` config
          ./base/configuration.nix
          ./base/system-packages.nix
          ./base/darwin.nix
          ./machine/m1laptop.nix
          inputs.nix-index-database.darwinModules.nix-index
          # `home-manager` module
          home-manager.darwinModules.home-manager
          {
            nixpkgs = nixpkgsConfig;
            # `home-manager` config
            home-manager.useGlobalPkgs = true;
            home-manager.useUserPackages = true;
            home-manager.users.nep = import ./base/home.nix;
          }
        ];
      };
    };

    # Overlays --------------------------------------------------------------- {{{

    overlays = {
        # Overlay useful on Macs with Apple Silicon
        apple-silicon = final: prev: optionalAttrs (prev.stdenv.system == "aarch64-darwin") {
          # Add access to x86 packages system is running Apple Silicon
          pkgs-x86 = import inputs.nixpkgs {
            system = "x86_64-darwin";
            inherit (nixpkgsConfig) config;
          };
        };
      };
 };
}
