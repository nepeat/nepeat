# sFlow for SONiC (docker-sflow, minus the container): hsflowd (host-sflow,
# the version sonic-buildimage pins in rules/sflow.mk, built FEATURES=SONIC:
# mod_sonic reads CONFIG_DB SFLOW / SFLOW_COLLECTOR and the port indexes,
# mod_psample takes the samples from the kernel's psample, mod_dropmon),
# sflowtool (the collector / decoder SONiC also packages; used by the tests)
# and port_index_mapper (STATE_DB PORT_INDEX_TABLE: port name -> index,
# ifindex). NixOS wiring: nix/modules/switch/sonic/sflow.nix.
#   s = sonic.sflow;  s.hsflowd  s.sflowtool  s.port-index-mapper
# Deviations: no mod_vpp (SONiC builds "SONIC VPP"); hsflowd's init script and
# systemd unit are not installed (the module has its own unit).
{ lib, stdenv, fetchFromGitHub, hiredis, autoreconfHook, python3, sonic }:
let
  py = python3.pkgs;
  spy = sonic.python;
  bi = sonic.sources.buildimage;
  meta = { homepage = "https://sflow.net"; platforms = lib.platforms.linux; };
in
{
  hsflowd = stdenv.mkDerivation {
    pname = "hsflowd";
    version = "2.1.26-1";
    src = fetchFromGitHub {
      owner = "sflow";
      repo = "host-sflow";
      rev = "v2.1.26-1";
      hash = "sha256-WvTozKsxAdMvcIYF3up3yNspDQwxbO8Cb/vIuvZmJFE=";
    };
    buildInputs = [ hiredis ];
    # the Makefiles call gcc / clang / ar by name
    postPatch = ''
      substituteInPlace src/sflow/Makefile --replace-fail 'ar cr' '$(AR) cr' --replace-fail 'ranlib $@' '$(RANLIB) $@'
    '';
    buildPhase = ''
      runHook preBuild
      make -C src/sflow CC="$CC -std=c99" AR=$AR RANLIB=$RANLIB
      make -C src/json libcjson.a CC="$CC -std=c89" AR=$AR
      make -C src/Linux FEATURES=SONIC VERSION=2.1.26 RELEASE=1 DEBIAN=no REDHAT=no \
        CC="$CC -std=gnu99" LD="$CC" MODDIR=$out/lib/hsflowd/modules -j$NIX_BUILD_CORES
      runHook postBuild
    '';
    installPhase = ''
      runHook preInstall
      install -Dm755 src/Linux/hsflowd $out/bin/hsflowd
      install -d $out/lib/hsflowd/modules
      install -m755 src/Linux/*.so $out/lib/hsflowd/modules/
      install -Dm644 src/Linux/scripts/hsflowd.conf.sonic $out/etc/hsflowd.conf
      runHook postInstall
    '';
    meta = meta // { description = "host-sflow agent, SONiC build (mod_sonic, mod_psample, mod_dropmon)"; license = lib.licenses.free; };   # InMon sFlow license
  };

  sflowtool = stdenv.mkDerivation {
    pname = "sflowtool";
    version = "6.11-c953cfe";
    src = fetchFromGitHub {
      owner = "sflow";
      repo = "sflowtool";
      rev = "c953cfe8991286457080938a29fc72019f64718a";   # sonic-buildimage src/sflow/sflowtool
      hash = "sha256-oOLXM9b+ncmyLS0JYcS4qzJ7lCCfc8xvVrdO2lYifm0=";
    };
    nativeBuildInputs = [ autoreconfHook ];
    meta = meta // { description = "sFlow collector / decoder"; license = lib.licenses.free; mainProgram = "sflowtool"; };
  };

  # STATE_DB PORT_INDEX_TABLE|<port>: index (from the name) and the netdev's
  # ifindex, kept current from STATE_DB PORT_TABLE / VLAN_TABLE.
  port-index-mapper = py.buildPythonPackage {
    pname = "sonic-port-index-mapper";
    version = sonic.version "buildimage";
    src = "${bi}/dockers/docker-sflow";
    format = "other";
    dontBuild = true;
    propagatedBuildInputs = [ spy.py-common spy.swsscommon ];
    installPhase = "install -Dm755 port_index_mapper.py $out/bin/port_index_mapper";
    meta = meta // { description = "SONiC port_index_mapper (docker-sflow)"; license = lib.licenses.asl20; };
  };
}
