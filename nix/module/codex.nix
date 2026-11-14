{ pkgs, inputs, lib, isStandalone, ... }:
let
  commonPrompt = builtins.readFile ./common/AGENTS.md;
  configPython = pkgs.python3.withPackages (ps: [ ps.tomlkit ]);

  codexHomeConfig = {
    home.packages = [ inputs.llm-agents.packages.${pkgs.stdenv.hostPlatform.system}.codex ];

    # Codex reads global instructions from ~/.codex/AGENTS.md, the same way
    # opencode reads ~/.config/opencode/AGENTS.md and claude reads
    # ~/.claude/CLAUDE.md. Same prompt file behind all three.
    home.file.".codex/AGENTS.md".text = commonPrompt;

    # Keep config.toml mutable for Codex; patch only the shared voice prompt.
    home.activation.codexOutputStyle = {
      after = [ "writeBoundary" ];
      before = [ ];
      data = ''
        run ${configPython}/bin/python3 - <<'PY'
        from pathlib import Path
        import tempfile
        import tomlkit

        settings = Path.home() / ".codex" / "config.toml"
        config = tomlkit.parse(settings.read_text()) if settings.exists() else tomlkit.document()
        config["developer_instructions"] = Path("${./common/feral-catgirl.md}").read_text()
        updated = tomlkit.dumps(config)
        if not settings.exists() or settings.read_text() != updated:
            settings.parent.mkdir(parents=True, exist_ok=True)
            with tempfile.NamedTemporaryFile(mode="w", dir=settings.parent, delete=False) as tmp:
                temporary = Path(tmp.name)
                try:
                    tmp.write(updated)
                    tmp.close()
                    if settings.exists():
                        temporary.chmod(settings.stat().st_mode & 0o777)
                    temporary.replace(settings)
                finally:
                    temporary.unlink(missing_ok=True)
        PY
      '';
    };
  };
in
if isStandalone then {
  # This branch is for standalone home-manager
  home = codexHomeConfig.home;
} else {
  # This branch is for nix-darwin or NixOS where the home-manager module is loaded
  home-manager.sharedModules = [ codexHomeConfig ];
}
