# SONiC's routing stack: FRR (bgpd, staticd, zebra, mgmtd, bfdd) with the FPM
# patches fpmsyncd needs, the FRR config templates of docker-fpm-frr, and
# bgpcfgd/bgpmon/staticroutebfd (sonic-bgpcfgd), which turn CONFIG_DB into FRR
# config, plus bfdsyncd (ours: bfdd session state -> STATE_DB).
#   sonic.routing.frr  sonic.routing.templates  sonic.routing.bgpcfgd  sonic.routing.bfdsyncd
# Deviations from sonic-buildimage, all here:
#  - FRR is nixpkgs' (same release as SONiC's src/sonic-frr) with only the
#    SONiC patches the FPM path needs (list below); not SONiC's full series.
#  - zebra uses FRR's own dplane_fpm_nl, not SONiC's dplane_fpm_sonic (a fork
#    that also needs libnexthopgroup from src/libraries/sonic-fib); the
#    difference is SRv6, EVPN-MH MACs and full next-hop groups, none of which
#    this stack runs. Plain routes use the same netlink encoding.
#  - Daemons trimmed to what SONiC's default (bgpcfgd) mode runs: zebra,
#    mgmtd, bgpd, staticd, bfdd (+ vtysh); no SNMP/RPKI/BMP/VNC/Lua.
#  - Software BFD (no ASIC offload): patches/0002 (BGP_NEIGHBOR bfd in the
#    bgpcfgd templates), patches/0003 (sonic-bgpcfgd: active sessions, live BFD
#    toggle, staticroutebfd timers from the environment) and bfdsyncd. SONiC
#    carries no bfdd patch in src/sonic-frr/patch, so FRR's bfdd is unpatched.
{ pkgs, sonic }:
let
  inherit (pkgs) lib;
  bi = sonic.sources.buildimage;
  spy = sonic.python;
  py = spy.python3.pkgs;
  # SONiC FRR patches (src/sonic-frr/patch, applied in series order):
  sonicFrrPatches = map (p: "${bi}/src/sonic-frr/patch/${p}.patch") [
    # FPM route messages carry the VRF id in rtm_table (fpmsyncd reads a
    # non-zero table as a VRF ifindex: the main table 254 would be dropped).
    "0008-SONiC-ONLY-Use-vrf_id-for-vrf-not-tabled_id"
    # No /32 "local" routes to the FPM (orchagent installs IP2ME itself).
    "0013-SONiC-ONLY-zebra-do-not-send-local-routes-to-fpm"
    # Route tag in RTA_PRIORITY of FPM messages (fpmsyncd's route tag).
    "0017-SONiC-ONLY-Patch-to-send-tag-value-associated-with-route-via-ne"
    "0026-SONiC-ONLY-Translate-tableid-for-dplane-route-notify"
  ];

  frr = (pkgs.frr.override {
    snmpSupport = false; rpkiSupport = false; scriptingSupport = false;
    watchfrrSupport = false;              # systemd restarts the daemons
    irdpSupport = false;
    numMultipath = 514;                   # as SONiC (patch 0016)
    bgpVnc = false; bgpBmp = false;       # SONiC: VNC off; BMP only with FEATURE bmp
    ripdSupport = false; ripngdSupport = false; ospfdSupport = false; ospf6dSupport = false;
    ldpdSupport = false; nhrpdSupport = false; eigrpdSupport = false; babeldSupport = false;
    isisdSupport = false; pimdSupport = false; pim6dSupport = false; sharpdSupport = false;
    fabricdSupport = false; vrrpdSupport = false; pathdSupport = false; bfddSupport = true;   # software BFD (services.sonic.bfd)
    pbrdSupport = false; ospfApi = false;
  }).overrideAttrs (old: {
    pname = "frr-sonic";
    patches = (old.patches or [ ]) ++ sonicFrrPatches
      # dplane_fpm_nl reads route notifications in that encoding (as dplane_fpm_sonic).
      ++ [ ./patches/0001-dplane-fpm-nl-route-notify-vrf-id.patch ];
    # Runtime only: headers, pkg-config and libtool files pulled the -dev
    # outputs and (version.h's configure line) the build machine's clippy
    # helper, 235 MiB of x86_64, into the switch closure.
    # libfrr also embeds the configure line ("show version"): strip the path.
    nativeBuildInputs = old.nativeBuildInputs ++ [ pkgs.buildPackages.removeReferencesTo ];
    postInstall = (old.postInstall or "") + ''
      rm -rf $out/include $out/lib/pkgconfig
      find $out/lib -name '*.la' -delete
    '';
    postFixup = (old.postFixup or "") + ''
      find $out -type f -exec remove-references-to -t ${old.clippy-helper} {} +
    '';
    disallowedReferences = [ old.clippy-helper ];
  });

  # docker-fpm-frr's templates (/usr/share/sonic/templates in the bgp
  # container). Absolute includes made relative: sonic-cfggen -T and
  # bgpcfgd's TemplateFabric both search this directory.
  templates = pkgs.runCommand "sonic-frr-templates-${sonic.version "buildimage"}" { } ''
    mkdir -p $out/share/sonic
    cp -r ${bi}/dockers/docker-fpm-frr/frr $out/share/sonic/frr-templates
    chmod -R u+w $out
    patch -p1 -d $out/share/sonic/frr-templates < ${./patches/0002-templates-bgp-neighbor-bfd.patch}
    grep -rl '/usr/share/sonic/templates/' $out | xargs sed -i 's|/usr/share/sonic/templates/||g'
    if grep -r '/usr/share/sonic/templates' $out; then exit 1; fi
  '';
  templateDir = "${templates}/share/sonic/frr-templates";

  bgpcfgd = spy.copyPkg {
    pname = "sonic-bgpcfgd";
    src = "${bi}/src/sonic-bgpcfgd";
    modules = [ "bgpcfgd" "bgpmon" "staticroutebfd" ];
    scripts = [ "nix-bin/bgpcfgd" "nix-bin/bgpmon" "nix-bin/staticroutebfd" ];
    deps = [ spy.py-common spy.swsscommon py.jinja2 py.netaddr py.pyyaml ];
    patches = [ ./patches/0003-bgpcfgd-software-bfd.patch ];
    postPatch = ''
      # BfdMgr starts bfdd itself when none runs (systemd runs it here: sonic-frr-bfdd).
      substituteInPlace bgpcfgd/managers_bfd.py --replace-fail '"/usr/lib/frr/bfdd"' '"${frr}/libexec/frr/bfdd"'
      substituteInPlace bgpcfgd/template.py \
        --replace-fail "template_path = '/usr/share/sonic/templates'" "template_path = '${templateDir}'"
      mkdir nix-bin
      for e in bgpcfgd:bgpcfgd.main staticroutebfd:staticroutebfd.main bgpmon:bgpmon.bgpmon; do
        printf '#!/usr/bin/env python3\nimport sys\nfrom %s import main\nsys.exit(main())\n' "''${e#*:}" > nix-bin/''${e%%:*}
      done
    '';
    imports = [ "bgpcfgd.main" "bgpmon.bgpmon" "staticroutebfd.main" ];
    passthru = { inherit templateDir; };
  };

  # FRR bfdd session state -> STATE_DB BFD_SESSION_TABLE (what BfdOrch writes
  # when the ASIC offloads BFD; ./bfdsyncd/bfdsyncd has the why).
  bfdsyncd = spy.copyPkg {
    pname = "sonic-nix-bfdsyncd";
    src = ./bfdsyncd;
    modules = [ ];
    scripts = [ "bfdsyncd" ];
    deps = [ spy.swsscommon ];
  };
  # /etc/sonic/constants.yml (sonic_debian_extension renders it with j2; the
  # only variable is the FRR SNMP agent, off here).
  constants = pkgs.runCommand "sonic-constants.yml" {
    nativeBuildInputs = [ (pkgs.buildPackages.python3.withPackages (p: [ p.jinja2 ])) ];
  } ''
    python3 -I -c 'import jinja2, sys; sys.stdout.write(jinja2.Template(open(sys.argv[1]).read(), keep_trailing_newline=True).render(ENABLE_FRR_SNMP_AGENT="n"))' \
      ${bi}/files/build_templates/constants.yml.j2 > $out
    grep -q 'enable_snmp_agent: false' $out
  '';
in {
  inherit frr templates templateDir bgpcfgd bfdsyncd sonicFrrPatches constants;
}
