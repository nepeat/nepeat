# Machine secrets for the AI agent tooling, rendered by vault-agent.
#
# Nothing here touches persistent disk except the openviking config, which has
# to live in erin's home because that is where ovcli looks. Everything else
# lands in /run.
#
# KV v2 mount `secret`, as module/openviking/setup-openviking-config.sh uses,
# so a logical path of `infra/openviking` is read as
# `secret/data/infra/openviking`.
#
# Everything lives under infra/ on purpose: that is what the `nixos-core`
# AppRole policy already grants (`secret/data/infra/*`), so machines read these
# without widening a policy shared with the rest of the fleet. The AI keys were
# moved out of app/ai/; openviking was COPIED from app/openviking, which still
# has readers (setup-openviking-config.sh, and an ArgoCD VaultSecret in the
# megarepo) -- so the two copies must be kept in sync by hand if rotated.
{
  lib,
  pkgs,
  config,
  ...
}:

let
  cfg = config.agentSecrets;

  # KV v2 hides the payload one level down: secret/app/x -> secret/data/app/x,
  # and the values sit under .Data.data.
  kv = path: "${cfg.mount}/data/${path}";

  # Field names are looked up with `index` rather than dotted access because
  # Go templates cannot parse a hyphen in a field name -- .Data.data.client-api-key
  # is a subtraction, not a lookup.
  field = f: ''{{ index .Data.data "${f}" }}'';
in

{
  options.agentSecrets = {
    enable = lib.mkEnableOption "vault-rendered secrets for the AI agent tooling";

    user = lib.mkOption {
      type = lib.types.str;
      default = "erin";
      description = "User the secrets are rendered for.";
    };

    group = lib.mkOption {
      type = lib.types.str;
      default = "users";
      description = "Group of the rendered secret files.";
    };

    mount = lib.mkOption {
      type = lib.types.str;
      default = "secret";
      description = "KV v2 mount holding these secrets.";
    };

    opencode = {
      goKeyPath = lib.mkOption {
        type = lib.types.str;
        default = "infra/ai/opencode";
        description = "Logical KV path for the opencode key (-> OPENCODE_GO_KEY).";
      };
      goKeyField = lib.mkOption {
        type = lib.types.str;
        default = "key";
        description = "Field within goKeyPath.";
      };
      openrouterPath = lib.mkOption {
        type = lib.types.str;
        default = "infra/ai/openrouter";
        description = "Logical KV path for the openrouter key (-> OPENROUTER_KEY).";
      };
      openrouterField = lib.mkOption {
        type = lib.types.str;
        default = "key";
        description = "Field within openrouterPath.";
      };
      envFile = lib.mkOption {
        type = lib.types.str;
        default = "/run/secrets/opencode.env";
        description = "Where the rendered env file lands. tmpfs, never persisted.";
      };

      authFile = lib.mkOption {
        type = lib.types.str;
        default = "/home/${cfg.user}/.local/share/opencode/auth.json";
        defaultText = lib.literalExpression ''"/home/''${cfg.user}/.local/share/opencode/auth.json"'';
        description = ''
          opencode's credential store, the file `/connect` writes. Managing it
          here means anything added interactively is OVERWRITTEN on the next
          render -- add further providers to `authProviders` instead.
        '';
      };

      authProviders = lib.mkOption {
        type = lib.types.attrsOf (
          lib.types.submodule {
            options = {
              path = lib.mkOption {
                type = lib.types.str;
                description = "Logical KV path holding this provider's key.";
              };
              field = lib.mkOption {
                type = lib.types.str;
                default = "key";
                description = "Field within that path.";
              };
            };
          }
        );
        default = {
          # OpenCode Zen and OpenCode Go are separate providers, billed
          # separately, but one key from opencode.ai/auth covers both.
          "opencode".path = "infra/ai/opencode";
          "opencode-go".path = "infra/ai/opencode";
          # Different secret entirely -- hence per-provider paths rather than
          # one path fanned out across a list of ids.
          "openrouter".path = "infra/ai/openrouter";
        };
        description = ''
          models.dev provider ids to write into auth.json, each with the KV
          path and field holding its key.
        '';
      };
    };

    openviking = {
      path = lib.mkOption {
        type = lib.types.str;
        default = "infra/openviking";
        description = "Logical KV path for the openviking client key.";
      };
      keyField = lib.mkOption {
        type = lib.types.str;
        default = "client-api-key";
        description = ''
          Field within path. Note this is the CLIENT key --
          setup-openviking-config.sh reads `root-api-key` from the same secret.
        '';
      };
      url = lib.mkOption {
        type = lib.types.str;
        default = "https://openviking.tail285d8f.ts.net";
        description = "OpenViking server URL.";
      };
      account = lib.mkOption {
        type = lib.types.str;
        default = "default";
      };
      account_user = lib.mkOption {
        type = lib.types.str;
        default = "default";
      };
      configFile = lib.mkOption {
        type = lib.types.str;
        default = "/home/${cfg.user}/.openviking/ovcli.conf";
        defaultText = lib.literalExpression ''"/home/''${cfg.user}/.openviking/ovcli.conf"'';
        description = "Where ovcli looks for its config.";
      };
    };
  };

  config = lib.mkIf cfg.enable {
    assertions = [
      {
        assertion = config.vaultAgent.enable;
        message = "agentSecrets renders through vault-agent; set vaultAgent.enable = true.";
      }
    ];

    # vault-agent runs as root and consul-template's `perms` only sets the mode,
    # not the owner, so each template chowns itself to the consuming user.
    vaultAgent.templates = {
      opencode = {
        destination = cfg.opencode.envFile;
        perms = "0400";
        contents = ''
          {{- with secret "${kv cfg.opencode.goKeyPath}" }}
          OPENCODE_GO_KEY=${field cfg.opencode.goKeyField}
          {{- end }}
          {{- with secret "${kv cfg.opencode.openrouterPath}" }}
          OPENROUTER_KEY=${field cfg.opencode.openrouterField}
          {{- end }}
        '';
        command = "${pkgs.coreutils}/bin/chown ${cfg.user}:${cfg.group} ${cfg.opencode.envFile}";
      };

      # opencode reads credentials from auth.json, not the environment, so the
      # env file above is not enough on its own.
      opencode-auth = {
        destination = cfg.opencode.authFile;
        perms = "0600";
        # One inline `with secret` per value rather than wrapping the whole
        # document: the providers live at different KV paths, and nesting
        # block-level `with` around JSON members mangles the commas.
        contents =
          let
            entry = prov: c:
              ''    "${prov}": { "type": "api", "key": "{{ with secret "${kv c.path}" }}{{ index .Data.data "${c.field}" }}{{ end }}" }'';
          in
          ''
            {
            ${lib.concatStringsSep ",\n" (lib.mapAttrsToList entry cfg.opencode.authProviders)}
            }
          '';
        command = "${pkgs.coreutils}/bin/chown ${cfg.user}:${cfg.group} ${cfg.opencode.authFile}";
      };

      openviking-cli = {
        destination = cfg.openviking.configFile;
        perms = "0600";
        contents = ''
          {{- with secret "${kv cfg.openviking.path}" }}
          {
            "url": "${cfg.openviking.url}",
            "api_key": "${field cfg.openviking.keyField}",
            "account": "${cfg.openviking.account}",
            "user": "${cfg.openviking.account_user}"
          }
          {{- end }}
        '';
        command = "${pkgs.coreutils}/bin/chown ${cfg.user}:${cfg.group} ${cfg.openviking.configFile}";
      };
    };

    # vault-agent will not create the parent directory itself.
    systemd.tmpfiles.rules = [
      "d /run/secrets 0755 root root"
      "d /home/${cfg.user}/.openviking 0700 ${cfg.user} ${cfg.group}"
      "d /home/${cfg.user}/.local 0755 ${cfg.user} ${cfg.group}"
      "d /home/${cfg.user}/.local/share 0755 ${cfg.user} ${cfg.group}"
      "d /home/${cfg.user}/.local/share/opencode 0700 ${cfg.user} ${cfg.group}"
    ];

    # The keys are env vars for an interactive CLI, so they have to reach the
    # shell. Sourced defensively: the file is absent until vault-agent has
    # authenticated, and a login shell must not break in the meantime.
    home-manager.users.${cfg.user}.programs.zsh.initContent = lib.mkOrder 1600 ''
      [ -r "${cfg.opencode.envFile}" ] && set -a && . "${cfg.opencode.envFile}" && set +a
    '';
  };
}
