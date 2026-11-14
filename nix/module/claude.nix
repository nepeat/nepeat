{ pkgs, inputs, lib, isStandalone, ... }:
let
  commonPrompt = builtins.readFile ./common/AGENTS.md;

  claudeHomeConfig = {
    home.packages = [ inputs.llm-agents.packages.${pkgs.stdenv.hostPlatform.system}.claude-code ];

    home.file.".claude/CLAUDE.md".text = commonPrompt;

    # Statusline: quota, path, model, context and OpenViking state.
    # `force` because the file may already be there as a hand-made symlink.
    home.file.".claude/statusline.mjs" = {
      source = ./claude/statusline.mjs;
      force = true;
    };

    # ~/.claude/settings.json is a mutable file that Claude Code and Orca both
    # write to, so home-manager cannot own it; patch the one key instead.  The
    # script forwards its payload to Orca's statusline hook, which this
    # replaces in the single statusLine slot.  Written as a literal DAG entry
    # (what lib.hm.dag.entryAfter returns) because `lib` here is not always
    # home-manager's.
    home.activation.claudeStatusLine = {
      after = [ "writeBoundary" ];
      before = [ ];
      data = ''
        settings="$HOME/.claude/settings.json"
        run mkdir -p "$HOME/.claude"
        [ -s "$settings" ] || run sh -c 'echo "{}" > "$1"' sh "$settings"
        if [ -z "''${DRY_RUN:-}" ]; then
          tmp="$(mktemp "$settings.XXXXXX")"
          if ${pkgs.jq}/bin/jq \
            --arg cmd '${pkgs.nodejs}/bin/node "$HOME/.claude/statusline.mjs"' \
            '.statusLine = { type: "command", command: $cmd }' \
            "$settings" > "$tmp"; then
            mv "$tmp" "$settings"
          else
            rm -f "$tmp"
            echo "claude: could not patch statusLine in $settings" >&2
          fi
        fi
      '';
    };
  };
in
if isStandalone then {
  # This branch is for standalone home-manager
  home = claudeHomeConfig.home;
} else {
  # This branch is for nix-darwin or NixOS where the home-manager module is loaded.
  # The binary cache lives in base/configuration.nix and nixos/base.nix now that
  # all three agents share one upstream.
  home-manager.sharedModules = [ claudeHomeConfig ];
}