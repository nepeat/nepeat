{ pkgs, lib, ... }:
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

    # ~/.ssh/rc only runs under a real sshd; environments like coder handle
    # ssh in-process, so also maintain the symlink from shell init. Skip when
    # SSH_AUTH_SOCK already points at the symlink (e.g. inside tmux), or we
    # would link the symlink to itself.
    programs.zsh.initContent = lib.mkOrder 550 ''
        if [ -n "$SSH_AUTH_SOCK" ] && [ -S "$SSH_AUTH_SOCK" ] \
            && [ "$SSH_AUTH_SOCK" != "$HOME/.ssh/ssh_auth_sock" ]; then
            ln -sf "$SSH_AUTH_SOCK" "$HOME/.ssh/ssh_auth_sock"
        fi
    '';

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
            # Don't pass one-shot profile guards into new panes. If the tmux
            # server is spawned from an already-initialized shell it inherits
            # these, so pane login shells reset PATH via /etc/profile but then
            # skip re-sourcing nix-daemon.sh/hm-session-vars.sh, losing
            # ~/.nix-profile/bin. Removing the guards lets each pane rebuild
            # the nix environment from scratch.
            set-environment -gr __ETC_PROFILE_NIX_SOURCED
            set-environment -gr __HM_SESS_VARS_SOURCED
            set-environment -gr __HM_ZSH_SESS_VARS_SOURCED
            # Keep SSH_AUTH_SOCK/SSH_AGENT_PID out of update-environment:
            # otherwise attaching copies the client's (soon-stale) socket into
            # the session environment, shadowing the global value above.
            set-option -g update-environment "DISPLAY KRB5CCNAME MSYSTEM SSH_ASKPASS SSH_CONNECTION WAYLAND_DISPLAY WINDOWID XAUTHORITY XDG_CURRENT_DESKTOP XDG_SESSION_DESKTOP XDG_SESSION_TYPE"
        '';
    };
}
