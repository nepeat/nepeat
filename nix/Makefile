update:
	nix flake update -vvv
	nix-channel --update
	nix build ".#darwinConfigurations.personal.system"
	sudo ./result/sw/bin/darwin-rebuild switch --flake ".#personal" -vL
	nix-store --gc

updateNew:
	nix flake update -vvv
	nix-channel --update
	nix build ".#darwinConfigurations.newPersonal.system"
	sudo ./result/sw/bin/darwin-rebuild switch --flake ".#newPersonal" -vL
	nix-store --gc
