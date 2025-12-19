update:
	nix flake update -vvv
	nix-channel --update
	nix build ".#darwinConfigurations.personal.system"
	sudo ./result/sw/bin/darwin-rebuild switch --flake ".#personal" -vL

updateNew:
	nix flake update -vvv
	nix-channel --update
	nix build --always-allow-substitutes --abort-on-warn --show-trace ".#darwinConfigurations.newPersonal.system"
	sudo ./result/sw/bin/darwin-rebuild switch --flake ".#newPersonal" -vL

clean:
	nix-store --gc
