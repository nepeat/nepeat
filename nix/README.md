# nix/
my nix files for my machines.

i have no idea what i'm doing.

## coming from my current or past company?
looking at this nix base repo after seeing my internal nix flake? turn your head away, this is honestly a trash fire. it "works" for me but it's probably not a fit for your configuration given how things move.

## deploying
```
nix build ".#darwinConfigurations.$machine.system"
./result/sw/bin/darwin-rebuild switch --flake ".#$machine"
```

## machines
- personal

## credits
- this gist for a quick and dirty base https://gist.github.com/jmatsushita/5c50ef14b4b96cb24ae5268dab613050
