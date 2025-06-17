update:
	nix flake update -vvv
	nix-channel --update
	nix build ".#darwinConfigurations.personal.system"
	sudo ./result/sw/bin/darwin-rebuild switch --flake ".#personal" -vL
	nix-store --gc

