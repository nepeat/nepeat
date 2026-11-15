# SONiC's platform side as Nix packages: the pmon daemons (xcvrd,
# thermalctld, psud, syseepromd from sonic-platform-daemons), the platform
# CLI scripts of sonic-utilities that read what they publish, and LLDP
# (lldpd, lldpmgrd, lldp_syncd, lldpshow). NixOS wiring:
# nix/modules/switch/sonic/{pmon,lldp}.nix.
#   p = sonic.platform;
#   p.pmon { platformPackages = [ <sonic_platform package> ]; }   bin/{xcvrd,thermalctld,psud,syseepromd}
#   p.cli  { platformPackages = [ ... ]; }    fanshow tempershow psushow sfpshow decode-syseeprom sfputil
#   p.lldpd  p.lldpmgrd  p.lldp-syncd  p.lldpshow  p.lldpTemplates
# Deviations: lldpd without SNMP; lldpshow and lldp_syncd call lldpctl
# directly (SONiC: docker exec into the lldp container, /usr/sbin/lldpctl).
{ lib, python3, lldpd, libxml2, runCommand, sonic }:
let
  spy = sonic.python;
  py = python3.pkgs;
  site = python3.sitePackages;
  pd = sonic.sources.platform-daemons;
  bi = sonic.sources.buildimage;
  ut = sonic.sources.utilities;
  ds = sonic.sources.dbsyncd;
  upstreamLldpd = lldpd;
  meta = { homepage = "https://github.com/sonic-net"; license = lib.licenses.asl20; platforms = lib.platforms.linux; };
  entry = name: module: fn: ''
    cat > $out/bin/${name} <<'EOF'
    #!/usr/bin/env python3
    import sys
    from ${module} import ${fn}
    sys.exit(${fn}())
    EOF
    chmod +x $out/bin/${name}
  '';
in rec {
  # lldpd 1.0.22 (nixpkgs) without SNMP (SONiC's snmp agent is not run; saves
  # net-snmp on the switch), with XML output (lldpshow's table parses
  # "lldpctl -f xml"; libxml2 is in the closure for lxml anyway), plus
  # SONiC's src/lldpd patch 0001 (lldpcli fails
  # for a port lldpd does not know, so lldpmgrd retries). SONiC's 0002
  # (separate netlink sockets) is upstream since.
  lldpd = upstreamLldpd.overrideAttrs (o: {
    pname = "lldpd-sonic";
    patches = (o.patches or [ ]) ++ [ "${bi}/src/lldpd/patch/0001-return-error-when-port-does-not-exist.patch" ];
    configureFlags = lib.remove "--with-snmp" o.configureFlags ++ [ "--without-snmp" "--with-xml" ];
    buildInputs = lib.filter (p: (p.pname or "") != "net-snmp") o.buildInputs ++ [ libxml2 ];
    preConfigure = "";
  });

  # xcvrd (package) + thermalctld, psud, syseepromd (scripts), with the
  # board's sonic_platform importable.
  pmon = { platformPackages ? [ ] }: py.buildPythonPackage {
    pname = "sonic-pmon";
    version = sonic.version "platform-daemons";
    src = pd;
    format = "other";
    dontBuild = true;
    propagatedBuildInputs = [ spy.py-common spy.platform-common spy.swsscommon spy.natsort ] ++ platformPackages;
    installPhase = ''
      mkdir -p $out/${site} $out/bin
      cp -r sonic-xcvrd/xcvrd $out/${site}/
      ${entry "xcvrd" "xcvrd.xcvrd" "main"}
      for d in thermalctld psud syseepromd; do install -m755 sonic-$d/scripts/$d $out/bin/; done
    '';
    pythonImportsCheck = [ "xcvrd.xcvrd" ];
    inherit meta;
  };

  # The sonic-utilities scripts behind "show platform fan|temperature|
  # psustatus|syseeprom" and "show interfaces transceiver ...", plus sfputil.
  cli = { platformPackages ? [ ] }: py.buildPythonPackage {
    pname = "sonic-platform-cli";
    version = sonic.version "utilities";
    src = ut;
    format = "other";
    dontBuild = true;
    propagatedBuildInputs = [ spy.utilities ] ++ platformPackages;
    installPhase = ''
      mkdir -p $out/bin
      for s in fanshow tempershow psushow sfpshow decode-syseeprom; do install -m755 scripts/$s $out/bin/; done
      ${entry "sfputil" "sfputil.main" "cli"}
    '';
    inherit meta;
  };

  # "show lldp neighbors|table": lldpshow with lldpctl run directly.
  lldpshow = py.buildPythonPackage {
    pname = "sonic-lldpshow";
    version = sonic.version "utilities";
    src = ut;
    format = "other";
    dontBuild = true;
    propagatedBuildInputs = [ spy.utilities ];
    postPatch = ''
      substituteInPlace scripts/lldpshow --replace-fail \
        "['sudo', 'docker', 'exec', '-i', 'lldp{}'.format(self.lldp_instance[lldp_instace_num]), 'lldpctl']" \
        "['${lldpd}/bin/lldpctl']"
    '';
    installPhase = "install -Dm755 scripts/lldpshow $out/bin/lldpshow";
    inherit meta;
  };

  # lldpmgrd (dockers/docker-lldp): per-port "lldpcli configure ports <port>
  # lldp portidsubtype local <alias>" once the port is up; lldpcli on PATH.
  lldpmgrd = py.buildPythonPackage {
    pname = "sonic-lldpmgrd";
    version = sonic.version "buildimage";
    src = "${bi}/dockers/docker-lldp";
    format = "other";
    dontBuild = true;
    propagatedBuildInputs = [ spy.py-common spy.swsscommon ];
    installPhase = "install -Dm755 lldpmgrd $out/bin/lldpmgrd";
    inherit meta;
  };

  # lldp_syncd (sonic-dbsyncd): lldpctl -f json -> APPL_DB LLDP_ENTRY_TABLE,
  # LLDP_LOC_CHASSIS (what SNMP/telemetry read).
  lldp-syncd = py.buildPythonPackage {
    pname = "sonic-lldp-syncd";
    version = sonic.version "dbsyncd";
    src = ds;
    format = "other";
    dontBuild = true;
    propagatedBuildInputs = [ spy.py-common spy.swsscommon ];
    postPatch = ''
      substituteInPlace src/lldp_syncd/daemon.py --replace-fail "'/usr/sbin/lldpctl'" "'${lldpd}/bin/lldpctl'" \
        --replace-fail "'/usr/sbin/lldpcli'" "'${lldpd}/bin/lldpcli'"
      ! grep -rn '/usr/s\?bin' src/lldp_syncd src/sonic_syncd
    '';
    installPhase = ''
      mkdir -p $out/${site} $out/bin
      cp -r src/lldp_syncd src/sonic_syncd $out/${site}/
      ${entry "lldp_syncd" "lldp_syncd.main" "main"}
    '';
    pythonImportsCheck = [ "lldp_syncd.daemon" ];
    inherit meta;
  };

  # lldpd.conf templates (hostname, port ID subtype, system description),
  # rendered by sonic-cfggen from CONFIG_DB as docker-lldp's start.sh does.
  lldpTemplates = runCommand "sonic-lldp-templates" { } ''
    mkdir -p $out
    cp ${bi}/dockers/docker-lldp/lldpd.conf.j2 ${bi}/dockers/docker-lldp/lldpdSysDescr.conf.j2 $out/
  '';
}
