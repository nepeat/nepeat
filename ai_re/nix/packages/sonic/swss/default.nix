# sonic-swss: orchagent, the *mgrd config managers, the *syncd kernel syncers,
# swssconfig. Upstream recipe: debian/rules (plain autotools + countersyncd).
# Deviations from upstream, all here:
#  - C++17 instead of -std=c++14: nixpkgs' gtest requires it (the daemons are
#    built the same way as the tests). -Werror off (newer GCC).
#  - protobuf is dash-api's (3.21, as Debian trixie), linked via pkg-config.
#  - Commands that the managers run (cfgmgr/shellcmd.h) point at store paths
#    for ip/bridge (iproute2), bash, echo, grep, teamd/teamdctl; iptables,
#    conntrack, brctl, ndisc6, sysctl and wpa_supplicant (NAT, VXLAN, ND
#    refresh, MACsec) are looked up on PATH: the service unit provides them.
#  - Lua scripts load from $out/share/swss (swss-common's SWSS_LUA_DIR hook);
#    swss-common's scripts are linked in there too.
#  - Not built/installed: countersyncd (Rust, cargo git dependency on
#    swss-common), restore_neighbors.py / bgp_eoiu_marker.py (warm reboot / BGP
#    helpers), netbouncer.json sample. STP: stpmgrd builds without stp_ipc.h.
#  - Tests (doCheck, native only): tests/ (swssnet/request_parser/quoted),
#    orchagent/p4orch/tests and tests/mock_tests (gtest against saivs).
{ lib, stdenv, autoreconfHook, autoconf-archive, pkg-config, python3
, hiredis, zeromq, boost, nlohmann_json, libuuid, zlib, jansson, jemalloc
, yaml-cpp, gtest, iproute2, bash, coreutils, gnugrep
, sonic
, doCheck ? stdenv.buildPlatform.canExecute stdenv.hostPlatform }:
let
  inherit (sonic) swss-common sairedis libteam dash-api;
  libnl = sonic.libnl-sonic;
  inherit (dash-api) protobuf;
in
stdenv.mkDerivation {
  pname = "sonic-swss";
  version = sonic.version "swss";
  src = sonic.sources.swss;
  # SAI attribute values through the union member of the attribute's width
  # (big-endian: e.g. every PVID read as 0). See the patch header;
  # ./check-sai-value-types.py SAI_INC_DIR orchagent ... re-finds them.
  patches = [
    ./patches/0001-orchagent-sai-attr-value-width.patch
    # Test-only: fpmsyncd test helpers (stack overflow, little-endian assumptions).
    ./patches/0002-tests-fpmsyncd-buffers-and-byte-order.patch
  ];

  nativeBuildInputs = [ autoreconfHook autoconf-archive pkg-config ]
    ++ lib.optional doCheck (python3.pythonOnBuildForHost.withPackages (p: [ p.lxml ]));
  buildInputs = [
    swss-common sairedis libteam dash-api libnl
    hiredis zeromq boost nlohmann_json libuuid zlib jansson protobuf jemalloc yaml-cpp
  ] ++ lib.optional doCheck gtest;

  postPatch = ''
    # automake >= 1.13 removed AM_CONFIG_HEADER.
    substituteInPlace configure.ac \
      --replace-fail 'AM_CONFIG_HEADER(config.h)' 'AC_CONFIG_HEADERS([config.h])' \
      --replace-fail '-std=c++14' '-std=c++17' \
      --replace-fail ' -I/usr/include/swss -I/usr/include"' '"' \
      --replace-fail 'PKG_CHECK_MODULES([JANSSON], [jansson])' \
                     'PKG_CHECK_MODULES([JANSSON], [jansson])
    PKG_CHECK_MODULES([PROTOBUF], [protobuf])'
    sed -i '/CFLAGS_COMMON+=" -Werror"/d' configure.ac
    sed -i 's/-lprotobuf/$(PROTOBUF_LIBS)/' orchagent/Makefile.am tests/mock_tests/Makefile.am

    substituteInPlace cfgmgr/shellcmd.h \
      --replace-fail '"/sbin/ip"' '"${iproute2}/bin/ip"' \
      --replace-fail '"/sbin/bridge"' '"${iproute2}/bin/bridge"' \
      --replace-fail '"/sbin/brctl"' '"brctl"' \
      --replace-fail '"/bin/echo"' '"${coreutils}/bin/echo"' \
      --replace-fail '"/bin/bash"' '"${bash}/bin/bash"' \
      --replace-fail '"/bin/grep"' '"${gnugrep}/bin/grep"' \
      --replace-fail '"/usr/bin/teamd"' '"${libteam}/bin/teamd"' \
      --replace-fail '"/usr/bin/teamdctl"' '"${libteam}/bin/teamdctl"' \
      --replace-fail '"/sbin/iptables"' '"iptables"' \
      --replace-fail '"/usr/sbin/conntrack"' '"conntrack"'
    substituteInPlace cfgmgr/nbrmgr.cpp --replace-fail '"/usr/bin/ndisc6"' '"ndisc6"'
    # iproute2 keeps its default tables under its own share/ on NixOS.
    substituteInPlace fpmsyncd/routesync.h --replace-fail '"/usr/share/iproute2/rt_protos"' '"${iproute2}/share/iproute2/rt_protos"'
  '' + (if doCheck then ''
    patchShebangs run-gtest-suite.py
    # The tests "#define private public" before including headers that pull in
    # libstdc++'s <mutex> (via protobuf); with GCC 16 that breaks std::mutex.
    # Parse <mutex> first.
    sed -i 's/^CXXFLAGS = -g -O0$/CXXFLAGS = -g -O0 -include mutex/' tests/mock_tests/Makefile.am
    grep -q 'include mutex' tests/mock_tests/Makefile.am
    # The tests expect the command strings of shellcmd.h; give them ours.
    find tests orchagent/p4orch/tests -name '*.cpp' -o -name '*.h' | xargs sed -i -E \
      -e 's|/sbin/ip\b|${iproute2}/bin/ip|g' -e 's|/sbin/bridge\b|${iproute2}/bin/bridge|g' \
      -e 's|/bin/bash\b|${bash}/bin/bash|g' -e 's|/bin/echo\b|${coreutils}/bin/echo|g' \
      -e 's|/bin/grep\b|${gnugrep}/bin/grep|g' -e 's|/usr/bin/teamd\b|${libteam}/bin/teamd|g' \
      -e 's|/usr/bin/teamdctl\b|${libteam}/bin/teamdctl|g' -e 's|/usr/bin/ndisc6\b|ndisc6|g'
    # teamsyncd and p4orch tests rely on the build host's installed
    # /var/run/redis/sonic-db/database_config.json; load the tests' copy.
    for f in tests/mock_tests/teamsync_ut.cpp:./database_config.json \
             orchagent/p4orch/tests/test_main.cpp:../../../tests/mock_tests/database_config.json; do
      cat >> ''${f%%:*} <<EOF
    namespace { struct NixDbConfig { NixDbConfig() { if (!swss::SonicDBConfig::isInit()) swss::SonicDBConfig::initialize("''${f#*:}"); } } nixDbConfig; }
    EOF
    done
  '' else ''
    # Tests are built by "make all"; skip them when they cannot run.
    sed -i 's/ tests / /' Makefile.am
    sed -i 's|^SUBDIRS = p4orch/tests|SUBDIRS =|' orchagent/Makefile.am
  '');

  configureFlags = [
    "--disable-static"
    "--with-libnl-3.0-inc=${lib.getDev libnl}/include/libnl3"
  ];
  env = {
    # AC_CHECK_FILE cannot run when cross-compiling; SONiC's STP is not used.
    ac_cv_file__usr_include_stp_ipc_h = "no";
    NIX_CFLAGS_COMPILE = toString [
      "-I${swss-common}/include/swss" "-I${swss-common}/include"
      "-I${sairedis}/include/sai"
      sonic.nhIdAlias
      # Our Lua scripts, not swss-common's (loadLuaScript is inline).
      "-DSWSS_LUA_DIR=\"${placeholder "out"}/share/swss/\""
    ];
  };
  enableParallelBuilding = true;

  inherit doCheck;
  checkPhase = ''
    runHook preCheck
    if ! make -k check -j$NIX_BUILD_CORES; then
      for f in $(find . -name test-suite.log); do echo "== $f"; tail -n 60 $f; done
      exit 1
    fi
    runHook postCheck
  '';

  postInstall = ''
    for f in ${swss-common}/share/swss/*; do
      [ -e $out/share/swss/$(basename $f) ] || ln -s $f $out/share/swss/
    done
  '';

  passthru = { inherit libteam dash-api; };

  meta = {
    description = "SONiC switch state service: orchagent, managers and syncers";
    homepage = "https://github.com/sonic-net/sonic-swss";
    license = lib.licenses.asl20;
    platforms = lib.platforms.linux;
    mainProgram = "orchagent";
  };
}
