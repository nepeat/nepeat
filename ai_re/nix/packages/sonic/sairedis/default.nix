# libsairedis, libsaimeta(data), libsaivs (virtual-switch SAI), syncd and the
# sai* tools; also installs the SAI headers it was built against.
# Upstream recipe: debian/rules with the "syncd vs" profiles.
# libsai = null: syncd on the virtual SAI (--with-sai=vs). libsai = a vendor
# libsai package (e.g. sai-openbcm): syncd links that instead, as for a
# hardware platform build (no --with-sai).
{ lib, stdenv, buildPackages, autoreconfHook, autoconf-archive, pkg-config, swig, python3
, doxygen, graphviz, which, hiredis, zeromq, boost, nlohmann_json, libuuid, zlib
, sonic, libsai ? null }:
let
  vendor = libsai != null;
  # libsai's own dependency (libsdk.so) must be findable when linking
  # executables against libsai; a cross ld does not follow DT_RUNPATH.
  libsdk = if vendor then libsai.passthru.libsdk or null else null;
in
stdenv.mkDerivation {
  pname = if vendor then "sonic-sairedis-${libsai.pname}" else "sonic-sairedis";
  version = sonic.version "sairedis";
  src = sonic.sources.sairedis;
  # Big-endian: SAI's mask (de)serializers swap bytes unconditionally.
  patches = [ ./patches/0001-SAI-meta-saiserialize-mask-byte-order.patch ];

  nativeBuildInputs = [
    autoreconfHook autoconf-archive pkg-config swig
    # configure (AX_PYTHON_DEVEL) still imports distutils; setuptools shims it.
    (buildPackages.python3.withPackages (p: [ p.setuptools ]))
    # SAI/meta generates saimetadata.c from the headers at build time.
    (buildPackages.perl.withPackages (p: [ p.XMLSimple ])) doxygen graphviz which
  ];
  buildInputs = [ sonic.swss-common sonic.libnl-sonic hiredis zeromq boost nlohmann_json libuuid zlib python3 ]
    ++ lib.optional vendor libsai;

  postPatch = ''
    patchShebangs SAI/meta stub.pl
    # Newer doxygen warns about obsolete Doxyfile tags; upstream fails on any warning.
    sed -i "s/ 2>&1 | perl -npe .*//" SAI/meta/Makefile
    # parse.pl's built-in line-based XML reader assumes the layout of older
    # doxygen output and scrambles cross-referenced tag values with current
    # doxygen. It has a real-parser mode (-s, XML::Simple); -S skips the style
    # checks of upstream's own headers.
    substituteInPlace SAI/meta/Makefile --replace-fail 'perl -I. parse.pl' 'perl -I. -MXML::Simple parse.pl -s -S'
    # In that mode blank-only text between two cross-references is dropped;
    # join text and references with blanks ...
    substituteInPlace SAI/meta/xmlutils.pm \
      --replace-fail 'return $item->{content} . $item->{ref}[0]->{content};' \
                     'return $item->{content} . " " . join(" ", map { $_->{content} } @{ $item->{ref} });' \
      --replace-fail '$content .= $ref if defined $ref;' \
                     '$content .= " " . $ref . " " if defined $ref;'
    # ... and collapse runs of blanks, which the tag grammar does not expect.
    substituteInPlace SAI/meta/parse.pl --replace-fail '$desc =~ s/@@/\n@@/g;' '$desc =~ s/[ \t]+/ /g; $desc =~ s/@@/\n@@/g;'
    # The generator spell-checks upstream's comments with /usr/bin/aspell; not our job.
    perl -0pi -e 's/sub RunAspell\n\{/sub RunAspell\n{\n    return;/' SAI/meta/aspellcheck.pl SAI/meta/style.pm
    # Unit tests need gtest in C++14 mode and a live redis; the stack is
    # exercised by NixOS VM tests instead.
    sed -i 's/ tests unittest//' Makefile.am
    sed -i 's/^SUBDIRS = \. tests/SUBDIRS = ./' syncd/Makefile.am
  '' + lib.optionalString vendor ''
    # The vendor-libsai version check is an AC_TRY_RUN without a
    # cross-compiling branch; give it one (native builds still run it).
    substituteInPlace configure.ac --replace-fail \
      'AC_MSG_ERROR("SAI headers API version and library version mismatch")])],' \
      'AC_MSG_ERROR("SAI headers API version and library version mismatch")],[AC_MSG_RESULT(cross-compiling: not run)])],'
  '';

  configureFlags = [
    "--disable-static"
    "--disable-python2"
  ] ++ lib.optional (!vendor) "--with-sai=vs" ++ [
    "--with-swss-common-inc=${sonic.swss-common}/include/swss"
    "--with-swss-common-lib=${sonic.swss-common}/lib"
  ];
  env = {
    NIX_CFLAGS_COMPILE = "-Wno-error -I${python3}/include/${python3.libPrefix} -I${sonic.swss-common}/include -I${lib.getDev sonic.libnl-sonic}/include/libnl3";
    # Several headers use uint32_t & co. without <cstdint> (fine with older libstdc++).
    CXXFLAGS = "-g -O2 -include cstdint -include sstream";
  } // lib.optionalAttrs (libsdk != null) {
    NIX_LDFLAGS = "-rpath-link ${libsdk}/lib";
  };
  makeFlags = [ "PYTHON3_BLDLIBRARY=-l${python3.libPrefix}" ];
  enableParallelBuilding = true;

  # Headers as debian/libsai{redis,metadata,vs}-dev.install ship them (swss
  # includes sairedis.h, sai_serialize.h, SaiAttributeList.h, ... from there).
  postInstall = ''
    mkdir -p $out/include/sai
    cp SAI/inc/*.h SAI/experimental/*.h SAI/meta/sai*.h $out/include/sai/
    cp lib/sairedis.h meta/sai*.h meta/Sai*.h meta/Meta.h meta/AttrKeyMap.h meta/MetaKeyHasher.h \
      meta/OidRefCounter.h meta/PortRelatedSet.h meta/Notification*.h meta/Globals.h $out/include/sai/
  '';

  meta = {
    description = if vendor then "SONiC SAI redis bridge with syncd on ${libsai.pname}"
      else "SONiC SAI redis bridge (sairedis, syncd, virtual-switch SAI)";
    homepage = "https://github.com/sonic-net/sonic-sairedis";
    license = lib.licenses.asl20;
    platforms = lib.platforms.linux;
  };
}
