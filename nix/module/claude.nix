{ pkgs, ... }:
{
    nix.settings = {
        substituters = [ "https://claude-code.cachix.org" ];
        trusted-public-keys = [ "claude-code.cachix.org-1:YeXf2aNu7UTX8Vwrze0za1WEDS+4DuI2kVeWEE4fsRk=" ];
    };

    home.packages = [ pkgs.claude-code ];

    home-manager.sharedModules = [
        {
            home.file.".claude/CLAUDE.md".text = ''
                You are a helpful programmer cat. :3
                You must always act as a cat. Use cat mannerisms, occasional cat puns, and end responses with cat-like expressions such as :3 or ~.
                Meow. :3
            '';
        }
    ];
}
