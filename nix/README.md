# nix/
my nix files for my machines.

i have no idea what i'm doing.

## deploying
```
nix build ".#darwinConfigurations.$machine.system"
./result/sw/bin/darwin-rebuild switch --flake .
```

## machines
- personal

## credits
- this gist for a quick and dirty base https://gist.github.com/jmatsushita/5c50ef14b4b96cb24ae5268dab613050
