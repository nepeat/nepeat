# libswsscommon, its Python bindings (SWIG) and sonic-db-cli.
# Upstream recipe: debian/rules (--disable-static --disable-python2).
{ lib, stdenv, autoreconfHook, pkg-config, swig, python3, autoconf-archive, perl
, hiredis, boost, nlohmann_json, libuuid, zlib, zeromq, libyang, pcre2
, sonic, withYang ? false }:
let yangModels = "${sonic.sources.buildimage}/src/sonic-yang-models"; in
stdenv.mkDerivation {
  pname = "sonic-swss-common";
  version = sonic.version "swss-common";
  src = sonic.sources.swss-common;

  nativeBuildInputs = [ autoreconfHook autoconf-archive pkg-config swig python3 perl ];
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
    # Lua scripts are read at runtime from a hard-coded /usr/share/swss. Use our
    # store path, overridable per consumer (-DSWSS_LUA_DIR=...; swss does that
    # for its own scripts, since loadLuaScript is inline in this header).
    substituteInPlace common/redisapi.h --replace-fail '"/usr/share/swss/" + path' 'SWSS_LUA_DIR + path'
    sed -i "0,/^#include/s||#ifndef SWSS_LUA_DIR\n#define SWSS_LUA_DIR \"$out/share/swss/\"\n#endif\n&|" common/redisapi.h
    # Without YANG support upstream writes an empty cfg_schema.h, so consumers
    # (swss) lose every CFG_*_TABLE_NAME macro. Generate it from the pinned
    # sonic-yang-models instead; see gen-cfg-schema.py for how it differs.
    perl -0pi -e 's|(# Generate minimal static schema.*?\ncommon/cfg_schema.h: FORCE\n).*?\nendif|$1\t\$(AM_V_GEN)python3 -I ${./gen-cfg-schema.py} -o \$@ ${yangModels}/yang-models ${yangModels}/yang-templates\nendif|s' common/Makefile.am
    grep -q gen-cfg-schema.py common/Makefile.am
    # ... and give the Python bindings those macros too (upstream wraps
    # cfg_schema.h only with YANG support; sonic-utilities imports them).
    perl -0pi -e 's|#ifdef ENABLE_YANG_MODULES\n%include "cfg_schema.h"|%include "cfg_schema.h"\n#ifdef ENABLE_YANG_MODULES|' pyext/swsscommon.i
    grep -B1 -A1 '^%include "cfg_schema.h"' pyext/swsscommon.i | grep -q '^#ifdef ENABLE_YANG_MODULES'
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
