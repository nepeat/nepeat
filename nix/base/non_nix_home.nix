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

    # On foreign distros the single-user Nix installer wires Nix onto PATH via
    # /etc/profile.d/nix.sh, an /etc/profile fragment. bash login shells source
    # /etc/profile, but zsh never does (and the Debian /etc/zsh/zprofile bridge
    # is absent here), so an ssh login lands in a zsh with no nix/devenv/direnv
    # on PATH. Bridge it for login shells the same way Debian's /etc/zsh/zprofile
    # normally would. NixOS/darwin already source /etc/profile, so this module —
    # scoped to standalone (non-NixOS) home configs — is the right place for it.
    programs.zsh.profileExtra = ''
        [ -r /etc/profile ] && emulate sh -c 'source /etc/profile'
    '';

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

    # Point tmux sessions at the persistent ssh-agent socket symlink above,
    # so panes/windows keep working after the originating ssh connection drops.
    programs.tmux = {
        enable = true;
        extraConfig = ''
            set-environment -g SSH_AUTH_SOCK "$HOME/.ssh/ssh_auth_sock"
        '';
    };
}
