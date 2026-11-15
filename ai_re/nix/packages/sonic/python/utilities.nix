# sonic-utilities (show, config, sonic-clear, intfutil, portconfig, ...).
# All Python packages are installed; the console_scripts SONiC uses for
# ports/VLAN/PortChannel/config are wrapped, plus upstream's scripts/.
# NixOS deviations (store paths in postPatch, logic in patches/):
#  - /usr/local/bin/sonic-cfggen, /usr/local/yang-models -> store paths.
#  - The saved config ("config save/load/reload" default) is
#    /var/lib/sonic/config_db.json instead of /etc/sonic/config_db.json.
#  - "config reload" hands over to the NixOS config loader when present
#    (patch 0001): it merges the Nix base with the file per
#    devices/sonic-nixos/config-ownership.md instead of loading the file raw.
#  - Upstream pins click < 8.2; nixpkgs' click is used (see the VM test).
{ lib, py, python3, sonic, spy }:
let
  src = sonic.sources.utilities;
  # Python scripts from scripts/ that show/config call for ports, VLANs,
  # PortChannels, neighbours and counters (upstream installs ~100, many bash).
  scripts = [ "intfutil" "portconfig" "portstat" "intfstat" "ipintutil" "nbrshow" "fdbshow"
              "port2alias" "db_migrator.py" "dropstat" "queuestat" ];
  consoleScripts = {
    show = "show.main:cli";
    config = "config.main:config";
    sonic-clear = "clear.main:cli";
    counterpoll = "counterpoll.main:cli";
    acl-loader = "acl_loader.main:cli";
  };
in py.buildPythonPackage {
  pname = "sonic-utilities";
  version = sonic.version "utilities";
  inherit src;
  format = "other";
  dontBuild = true;
  patches = [
    ./patches/0001-config-reload-nixos-loader.patch
    # Alias naming mode (Ethernet<cage>/<sub>): portchannel member add/del
    # wrote the alias as the member key; run_command(return_cmd=True)
    # returned None in alias mode. Upstreamable.
    ./patches/0002-alias-mode-portchannel-member-and-return-cmd.patch
  ];

  postPatch = ''
    # main.py, config_mgmt.py and several plugins load sonic-cfggen as a module.
    for f in $(grep -rl "load_module_from_source('sonic_cfggen', '/usr/local/bin/sonic-cfggen')" --include='*.py' .); do
      substituteInPlace $f \
        --replace-fail "load_module_from_source('sonic_cfggen', '/usr/local/bin/sonic-cfggen')" \
                       "load_module_from_source('sonic_cfggen', '${spy.config-engine}/lib/sonic-cfggen/sonic-cfggen')"
    done
    ! grep -rn "'/usr/local/bin/sonic-cfggen'" --include='*.py' . | grep -v "SONIC_CFGGEN_PATH = \|tests/"
    substituteInPlace config/main.py \
      --replace-fail "SONIC_CFGGEN_PATH = '/usr/local/bin/sonic-cfggen'" "SONIC_CFGGEN_PATH = '${spy.config-engine}/bin/sonic-cfggen'" \
      --replace-fail "DEFAULT_CONFIG_DB_FILE = '/etc/sonic/config_db.json'" "DEFAULT_CONFIG_DB_FILE = '/var/lib/sonic/config_db.json'" \
      --replace-fail "with open('/etc/sonic/config_db.json') as f:" "with open(DEFAULT_CONFIG_DB_FILE) as f:"
    substituteInPlace generic_config_updater/main.py \
      --replace-fail "DEFAULT_CONFIG_DB_FILE = '/etc/sonic/config_db.json'" "DEFAULT_CONFIG_DB_FILE = '/var/lib/sonic/config_db.json'"
    for f in config/config_mgmt.py generic_config_updater/gu_common.py; do
      substituteInPlace $f --replace-fail 'YANG_DIR = "/usr/local/yang-models"' 'YANG_DIR = "${spy.yang-models}/share/sonic/yang-models"'
    done
  '';

  propagatedBuildInputs = [
    spy.py-common spy.config-engine spy.yang-mgmt spy.platform-common spy.swsscommon spy.natsort
  ] ++ (with py; [
    click tabulate netaddr netifaces jsonpatch jsonpointer jsondiff ijson xmltodict
    lazy-object-proxy pyyaml jinja2 lxml redis pyroute2
  ]);
  # Not included (no import on the show/config paths used here; upstream
  # lists them): bcrypt, cryptography, m2crypto, docker, paramiko, scp,
  # pexpect, requests, filelock, semantic-version, toposort, enlighten.

  installPhase = ''
    runHook preInstall
    site=$out/${python3.sitePackages}
    mkdir -p $site $out/bin
    python3 -I - "$site" <<'EOF'
    import ast, shutil, sys, os
    tree = ast.parse(open('setup.py').read())
    pkgs = next(k.value for n in ast.walk(tree) if isinstance(n, ast.Call) for k in n.keywords if k.arg == 'packages')
    for p in ast.literal_eval(pkgs):
        if p.split('.')[0] == 'tests':
            continue
        if '.' not in p:
            shutil.copytree(p, os.path.join(sys.argv[1], p))
    EOF
    cp show/aliases.ini $site/show/ 2>/dev/null || true
    for s in ${lib.escapeShellArgs scripts}; do install -m755 scripts/$s $out/bin/; done
    ${lib.concatStrings (lib.mapAttrsToList (name: ep: let m = lib.splitString ":" ep; in ''
      cat > $out/bin/${name} <<'EOF'
      #!/usr/bin/env python3
      import sys
      from ${lib.head m} import ${lib.last m}
      sys.exit(${lib.last m}())
      EOF
      chmod +x $out/bin/${name}
    '') consoleScripts)}
    runHook postInstall
  '';

  # Import the CLI trees: catches a missing dependency at build time. They
  # read the DB map at import time; give them swss-common's sample one.
  postFixup = lib.optionalString (python3.stdenv.buildPlatform.canExecute python3.stdenv.hostPlatform) ''
    cd $TMPDIR
    PYTHONPATH=$out/${python3.sitePackages}:$PYTHONPATH python3 -c "
    from swsscommon import swsscommon
    swsscommon.SonicDBConfig.initialize('${sonic.swss-common}/share/swsscommon/sonic-db/database_config.json')
    import show.main, config.main, utilities_common.cli
    print('sonic-utilities: show/config import OK')"
  '';
  # Unit tests need a mock redis tree and are not run.
  doCheck = false;

  meta = {
    description = "SONiC command line utilities (show, config, ...)";
    homepage = "https://github.com/sonic-net/sonic-utilities";
    license = lib.licenses.asl20;
    platforms = lib.platforms.linux;
  };
}
