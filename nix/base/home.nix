{ config, pkgs, lib, ... }:

{
  home.stateVersion = "26.05";

    # shell stuff
    programs.bash = {
        enable = true;
        profileExtra = (builtins.readFile ./dotfiles/profile);
    };

    programs.zsh = {
        enable = true;
        oh-my-zsh = {
            enable = true;
            plugins = [ "git" "thefuck" "direnv" ];
            theme = "gentoo";
        };

        initContent = lib.mkOrder 1500 ''
        . "$HOME/.profile"
        [ -f "$HOME/.zshrc-internal" ] && . "$HOME/.zshrc-internal"
        '';
    };

    # shell history
    programs.atuin = {
        enable = true;
    };

  # https://github.com/malob/nixpkgs/blob/master/home/default.nix

  # Direnv, load and unload environment variables depending on the current directory.
  # https://direnv.net
  # https://rycee.gitlab.io/home-manager/options.html#opt-programs.direnv.enable
  programs.direnv.enable = true;
  programs.direnv.nix-direnv.enable = true;

  # Htop
  # https://rycee.gitlab.io/home-manager/options.html#opt-programs.htop.enable
  programs.htop.enable = true;
  programs.htop.settings.show_program_path = true;

  home.packages = with pkgs; [
    # Some basics
    coreutils
    curl
    wget

    # flexing
    fastfetch
    pfetch

    # good utils
    cmake
    ripgrep
    exiftool
    dyff
    jq
    yq
    aria2
    nmap
    parallel
    smartmontools
    wget
    rsync
    wrk
    gnupg
    iperf3
    imagemagick
    graphviz
    croc
    pv
    rclone
    watch
    inetutils
    grepcidr
    mtr
    pay-respects
    yt-dlp
    sops

    # Dev stuff

    # python
    python314
    uv

    ## local db
    postgresql_18_jit

    ## nodejs
    nodePackages.typescript
    nodePackages.pnpm
    nodejs_22

    ## go
    go

    ## rust
    rustc
    cargo

    ## ruby
    ruby_3_5

    ## java
    temurin-bin-21

    ## cloud providers
    oci-cli
    google-cloud-sdk

    ## k8s
    kubectl
    kustomize
    kubeseal
    kubernetes-helm
    talhelper

    ## hashicorp + forks
    opentofu
    packer
    (openbao.override { withUi = false; })

    ## c + misc
    pkg-config

    ## override macos specific stuff
    gnutar
    bash

    ## misc
    ffmpeg_7
    valkey
    zstd
    git-secret
    p7zip
    protobuf
    tmux
    just
    bitwarden-cli
    pre-commit

    # Useful nix related tools
    cachix # adding/managing alternative binary caches hosted by Cachix
    comma # run software from without installing it
    niv # easy dependency management for nix projects
  ] ++ lib.optionals stdenv.isDarwin [
    cocoapods
    m-cli # useful macOS CLI commands
  ];

  programs.ssh = {
    enable = true;
    enableDefaultConfig = false;
    includes = [
        "conf.d/*"
    ];
    matchBlocks = {
        "*" = {
            kexAlgorithms = ["+diffie-hellman-group1-sha1"];
            extraOptions = {
                "Ciphers" = "+aes128-cbc";
                "HostKeyAlgorithms" = "+ssh-rsa";
                "PubkeyAcceptedAlgorithms" = "+ssh-rsa";
                "StrictHostKeychecking" = "no";
            };
        };
    };
  };

  programs.git = {
    enable = true;

    settings = {
        user = {
            name = lib.mkDefault "nepeat";
            email = lib.mkDefault "nepeat@gmail.com";
            signingkey = lib.mkDefault "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIPnTPYFFfFpbQ5vaBYdlScyGO76LByosMax56GsWUCsy";
        };

        alias = {
            co = "checkout";
            ci = "commit";
            st = "status";
            pl = "pull";
            plr = "pull --rebase";
            br = "branch";
            ps = "push";
            psr = "push origin HEAD:refs/for/master%r=erin.liman";
            dt = "difftool";
            l = "log --stat";
            cp = "cherry-pick";
            ca = "commit -a";
        };

        commit = {
            gpgsign = lib.mkDefault "true";
        };

        gpg = {
            format = "ssh";
        };

        "gpg \"ssh\"" = lib.mkDefault {
            program = "/Applications/1Password.app/Contents/MacOS/op-ssh-sign";
        };

        extraConfig = {
            push = {
                default = "simple";
            };

            merge = {
                tool = "opendiff";
            };

            http = {
                sslverify = "false";
            };

            "filter \"lfs\"" = {
                required = "true";
                clean = "git-lfs clean -- %f";
                smudge = "git-lfs smudge -- %f";
                process = "git-lfs filter-process";
            };
        };
    };

    includes = [
        { path = "~/.gitconfig-internal"; }
    ];

    ignores = [
        ".DS_Store"
    ];
  };
}
