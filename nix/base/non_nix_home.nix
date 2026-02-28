{ pkgs, ... }:
{
    nix.package = pkgs.lix;

    # hijack bash to run zsh
    programs.bash = {
        enable = true;
        initExtra = ''
            if [[ $- == *i* ]]; then
                if command -v zsh > /dev/null 2>&1; then
                    exec zsh
                fi
            fi
        '';
    };
}
