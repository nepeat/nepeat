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

    # SSH agent socket persistence across tmux/screen sessions
    home.file.".ssh/rc" = {
        text = ''
            if test "$SSH_AUTH_SOCK"; then
                ln -sf $SSH_AUTH_SOCK ~/.ssh/ssh_auth_sock
            fi
        '';
        executable = true;
    };

    home.file.".ssh/conf.d/agent-socket.conf" = {
        text = ''
            Host *
                IdentityAgent ~/.ssh/ssh_auth_sock
        '';
    };
}
