# libswsscommon, its Python bindings (SWIG) and sonic-db-cli.
# Upstream recipe: debian/rules (--disable-static --disable-python2).
{ lib, stdenv, autoreconfHook, pkg-config, swig, python3, autoconf-archive
, hiredis, boost, nlohmann_json, libuuid, zlib, zeromq, libyang, pcre2
, sonic, withYang ? false }:
stdenv.mkDerivation {
  pname = "sonic-swss-common";
  version = sonic.version "swss-common";
  src = sonic.sources.swss-common;

  nativeBuildInputs = [ autoreconfHook autoconf-archive pkg-config swig python3 ];
  buildInputs = [ hiredis sonic.libnl-sonic boost nlohmann_json libuuid zlib zeromq python3 ]
    ++ lib.optionals withYang [ libyang pcre2 ];

  configureFlags = [
    "--disable-static"
    "--disable-python2"
    (lib.enableFeature withYang "yangmodules")
  ];
  # The unit tests are built by "make all" with -std=c++14, which nixpkgs'
  # gtest (>= 1.17, C++17 only) rejects; they also need a live redis. Not
  # built; the stack is exercised by the NixOS VM tests instead.
  postPatch = ''
    sed -i '/include tests\/Makefile.am/d' Makefile.am
  '';

  # Upstream builds with -Werror against Debian's compiler; newer GCCs add warnings.
  # pyext hard-codes -I/usr/include/python3.x and links no libpython.
  env.NIX_CFLAGS_COMPILE = "-Wno-error -I${python3}/include/${python3.libPrefix}";
  makeFlags = [ "PYTHON3_BLDLIBRARY=-l${python3.libPrefix}" ];

  # database_config.json installs to the absolute runtime path
  # /var/run/redis/sonic-db; the NixOS module renders the real one there.
  installFlags = [
    "swsscommondir=${placeholder "out"}/share/swsscommon/sonic-db"
    # AM_PATH_PYTHON3 points these into the python interpreter's own prefix.
    "pkgpython3dir=${placeholder "out"}/${python3.sitePackages}/swsscommon"
    "pkgpy3execdir=${placeholder "out"}/${python3.sitePackages}/swsscommon"
  ];
  # Headers are shipped by debian/libswsscommon-dev.install, not "make install".
  postInstall = ''
    mkdir -p $out/include/swss
    cp common/*.h common/*.hpp $out/include/swss/
    cp pyext/swsscommon.i $out/include/swss/
  '';
  enableParallelBuilding = true;

  meta = {
    description = "SONiC switch state service common library";
    homepage = "https://github.com/sonic-net/sonic-swss-common";
    license = lib.licenses.asl20;
    platforms = lib.platforms.linux;
  };
}
