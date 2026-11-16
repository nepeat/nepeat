# hardware.cs8210.macsec: MACsec on the CS8210's Credo PHYs (ports 1-16).
#
#   * shim/: phy_server plugin (LD_PRELOAD) adding cmx_* commands that drive
#     the EIP-163/164 MACsec engine through the vendor's own DDK
#     (libdriver_164_u.so): SA add/switch/remove, next PN, counters, EAPOL
#     bypass, fail-closed data gating. glibc 2.17 symbols only.
#   * cs8210-macsec.py: CLI (status = "MACsec info", metrics, static keys,
#     hitless rekey, PN get/set).
#   * wpa_supplicant/: an MKA backend (-D macsec_cs8210) for wpa_supplicant
#     2.12, built as a separate package (wpa_supplicant-cs8210).
#   * test/run-test.sh: offline shim + client test against a stub DDK.
#
# Keys and CAKs live outside the store (root-only files named here by path).
# Capabilities, design and results: ai_re/devices/cs8210-32x/macsec.md.
{
  config,
  lib,
  pkgs,
  ...
}:

let
  cfg = config.hardware.cs8210.macsec;
  mon = config.monitoring.nodeExporter;
  sdkTmp = "/run/cs8210-sdk/tmp";

  shim = pkgs.stdenv.mkDerivation {
    pname = "cs8210-macsec-shim";
    version = "1";
    src = ./shim;
    # Loaded into the vendor's glibc 2.25 phy_server: no fortify, no stack
    # protector, nothing newer than GLIBC_2.17, libc the only dependency.
    hardeningDisable = [ "all" ];
    dontConfigure = true;
    buildPhase = ''
      $CC -std=gnu11 -O2 -Wall -Wextra -Werror -fPIC -shared -U_FORTIFY_SOURCE \
        -fno-stack-protector -Wl,-soname,libcs8210-macsec-shim.so \
        -o libcs8210-macsec-shim.so cs8210-macsec-shim.c
      if $READELF -V libcs8210-macsec-shim.so | grep -o 'GLIBC_[0-9.]*' | sort -u | grep -v '^GLIBC_2.17$'; then
        echo "shim references glibc symbols newer than 2.17" >&2
        exit 1
      fi
      if $READELF -d libcs8210-macsec-shim.so | grep NEEDED | grep -v 'libc.so.6'; then
        echo "shim needs more than libc" >&2
        exit 1
      fi
    '';
    installPhase = ''
      install -D -m 0644 libcs8210-macsec-shim.so $out/lib/libcs8210-macsec-shim.so
    '';
  };

  pyEnv = pkgs.python3.withPackages (ps: [ ps.cryptography ]);
  tool = pkgs.runCommand "cs8210-macsec" { } ''
    install -D -m 0755 ${./cs8210-macsec.py} $out/bin/cs8210-macsec
    sed -i '1s|.*|#!${pyEnv}/bin/python3|' $out/bin/cs8210-macsec
  '';

  wpa = pkgs.callPackage ./wpa_supplicant/package.nix { };

  csindex = {
    "gcm-aes-128" = 0;
    "gcm-aes-256" = 1;
  };
  mkaConf = pkgs.writeShellScript "cs8210-macsec-mka-conf" ''
    # $1 = interface. CAK/CKN from the root-only cakFile into a root-only
    # runtime file, never into the store.
    set -eu
    umask 077
    f=${toString cfg.mka.cakFile}
    case "$(stat -c %a "$f")" in 600|400) ;; *) echo "$f must be mode 600 or 400" >&2; exit 1 ;; esac
    cak=$(sed -n 's/^cak=\([0-9a-fA-F]*\)$/\1/p' "$f")
    ckn=$(sed -n 's/^ckn=\([0-9a-fA-F]*\)$/\1/p' "$f")
    case ''${#cak} in 32|64) ;; *) echo "cak= must be 32 or 64 hex digits" >&2; exit 1 ;; esac
    [ -n "$ckn" ] || { echo "ckn= missing" >&2; exit 1; }
    mkdir -p /run/cs8210-macsec
    cat > /run/cs8210-macsec/wpa-$1.conf <<EOF
    ap_scan=0
    network={
      key_mgmt=NONE
      eapol_flags=0
      macsec_policy=1
      macsec_integ_only=${if cfg.mka.integrityOnly then "1" else "0"}
      macsec_csindex=${toString csindex.${cfg.mka.cipher}}
      macsec_replay_protect=1
      macsec_replay_window=${toString cfg.mka.replayWindow}
      mka_priority=${toString cfg.mka.priority}
      mka_cak=$cak
      mka_ckn=$ckn
    }
    EOF
  '';
in
{
  options.hardware.cs8210.macsec = {
    enable = lib.mkEnableOption ''
      the MACsec shim in the vendor phy_server (LD_PRELOAD, takes effect at
      the next cs8210-switch start) and the cs8210-macsec tool. Ports stay
      in the vendor bypass until something configures them'';
    static.configFile = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = null;
      example = "/var/lib/cs8210-macsec/static.json";
      description = ''
        Root-only JSON with static SAs (format: cs8210-macsec --help;
        "cs8210-macsec keygen" writes one). Loaded by cs8210-macsec-static
        after every cs8210-switch start. Not in the Nix store: it holds keys.
      '';
    };
    mka = {
      ports = lib.mkOption {
        type = lib.types.listOf (lib.types.ints.between 1 16);
        default = [ ];
        example = [ 1 16 ];
        description = "Ports (fppN) running MKA (wpa_supplicant -D macsec_cs8210) while cs8210-switch is up.";
      };
      cakFile = lib.mkOption {
        type = lib.types.nullOr lib.types.str;
        default = null;
        example = "/var/lib/cs8210-macsec/mka-psk";
        description = "Root-only file with lines cak=HEX (32 or 64 digits) and ckn=HEX: the pre-shared CAK.";
      };
      cipher = lib.mkOption {
        type = lib.types.enum [ "gcm-aes-128" "gcm-aes-256" ];
        default = "gcm-aes-256";
        description = ''
          MKA cipher suite. wpa_supplicant 2.12 has no XPN suites, so the PN
          is 32 bits: at 400G line rate the key server rekeys on PN
          exhaustion (0xC0000000) about every 100 s with 1500 B frames.
        '';
      };
      integrityOnly = lib.mkOption {
        type = lib.types.bool;
        default = false;
        description = "macsec_integ_only: authenticate, do not encrypt.";
      };
      replayWindow = lib.mkOption {
        type = lib.types.ints.unsigned;
        default = 0;
      };
      priority = lib.mkOption {
        type = lib.types.ints.between 0 255;
        default = 255;
        description = "MKA key server priority (lower wins).";
      };
    };
    packages = lib.mkOption {
      internal = true;
      readOnly = true;
      default = { inherit shim tool wpa; };
      description = "The shim, the cs8210-macsec tool and wpa_supplicant-cs8210 (for building/testing).";
    };
    metrics = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = "cs8210_macsec_* metrics (counters, PN usage; no keys) as a node-exporter textfile producer.";
    };
  };

  config = lib.mkIf cfg.enable (lib.mkMerge [
    {
      assertions = [
        {
          assertion = config.hardware.cs8210.vendorSdk.enable;
          message = "hardware.cs8210.macsec needs hardware.cs8210.vendorSdk";
        }
        {
          assertion = cfg.mka.ports == [ ] || cfg.mka.cakFile != null;
          message = "hardware.cs8210.macsec.mka.ports needs mka.cakFile";
        }
      ];
      environment.systemPackages = [ tool ];
      # A stable path for the unit: a store path there would change the
      # cs8210-switch unit, and so restart (reset) the ASIC, on every rebuild.
      environment.etc."cs8210-macsec/libcs8210-macsec-shim.so".source = "${shim}/lib/libcs8210-macsec-shim.so";
      hardware.cs8210.vendorSdk.phyPreload = "/etc/cs8210-macsec/libcs8210-macsec-shim.so";
      systemd.tmpfiles.rules = [ "d /var/lib/cs8210-macsec 0700 root root - -" ];
    }

    (lib.mkIf (cfg.static.configFile != null) {
      systemd.services.cs8210-macsec-static = {
        description = "CS8210: static MACsec SAs from ${cfg.static.configFile}";
        wantedBy = [ "cs8210-switch.service" ];
        after = [ "cs8210-switch.service" ];
        partOf = [ "cs8210-switch.service" ];
        serviceConfig = {
          Type = "oneshot";
          RemainAfterExit = true;
          ExecStart = "${tool}/bin/cs8210-macsec up --config ${cfg.static.configFile}";
        };
      };
    })

    (lib.mkIf (cfg.mka.ports != [ ]) {
      systemd.services = lib.listToAttrs (
        map (
          p:
          lib.nameValuePair "cs8210-macsec-mka-fpp${toString p}" {
            description = "CS8210: MKA (802.1X-2010) on fpp${toString p}";
            wantedBy = [ "cs8210-switch.service" ];
            after = [ "cs8210-switch.service" ];
            partOf = [ "cs8210-switch.service" ];
            serviceConfig = {
              ExecStartPre = "${mkaConf} fpp${toString p}";
              ExecStart = "${wpa}/bin/wpa_supplicant -D macsec_cs8210 -i fpp${toString p} -c /run/cs8210-macsec/wpa-fpp${toString p}.conf";
              Restart = "on-failure";
              RestartSec = 5;
              UMask = "0077";
            };
          }
        ) cfg.mka.ports
      );
    })

    (lib.mkIf (cfg.metrics && mon.enable) {
      monitoring.nodeExporter.textfile.producers.cs8210-macsec = {
        command = "${tool}/bin/cs8210-macsec metrics";
        interval = 30;
        timeout = 20;
        serviceConfig = {
          ReadWritePaths = [
            mon.textfile.directory
            sdkTmp
          ];
          LogLevelMax = "notice";
        };
      };
    })
  ]);
}
