{ config, pkgs, lib, ... }:

{
  home.stateVersion = "23.11";

    # shell stuff
    programs.bash = {
        enable = true;
        profileExtra = (builtins.readFile ./dotfiles/profile);
    };

    programs.zsh = {
        enable = true;
        oh-my-zsh = {
            enable = true;
            plugins = [ "git" "ssh-agent" "thefuck" "direnv" ];
            theme = "gentoo";
        };

        initExtra = ''
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
    hugo
    pay-respects
    yt-dlp
    sops

    # Dev stuff

    ## local db
    postgresql_16_jit

    ## nodejs
    nodePackages.typescript
    nodePackages.pnpm
    nodejs_22
    nodePackages.node2nix
    bun

    ## go
    go

    ## rust
    rustc
    cargo

    ## ruby
    ruby_3_5

    ## android
    android-tools

    ## java
    temurin-bin-21

    ## c#
    mono

    ## cloud providers
    oci-cli
    google-cloud-sdk

    ## k8s
    kubectl
    kustomize
    kubeseal
    kubernetes-helm

    ## hashicorp + forks
    vault
    opentofu
    packer
    openbao

    ## misc
    consul-template
    ffmpeg_6
    redis
    openssh
    idris2
    purescript
    twilio-cli
    zstd
    qemu
    git-secret
    p7zip
    protobuf
    httpie
    websocat

    # Useful nix related tools
    cachix # adding/managing alternative binary caches hosted by Cachix
    comma # run software from without installing it
    niv # easy dependency management for nix projects
  ] ++ lib.optionals stdenv.isDarwin [
    cocoapods
    m-cli # useful macOS CLI commands
  ];

  programs.git = {
    enable = true;

    settings = {
        user = {
            name = "nepeat";
            email = "nepeat@gmail.com";
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
