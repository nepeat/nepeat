# libsairedis, libsaimeta(data), libsaivs (virtual-switch SAI), syncd and the
# sai* tools; also installs the SAI headers it was built against.
# Upstream recipe: debian/rules with the "syncd vs" profiles.
{ lib, stdenv, buildPackages, autoreconfHook, autoconf-archive, pkg-config, swig, python3
, doxygen, graphviz, which, hiredis, zeromq, boost, nlohmann_json, libuuid, zlib
, sonic }:
stdenv.mkDerivation {
  pname = "sonic-sairedis";
  version = sonic.version "sairedis";
  src = sonic.sources.sairedis;

  nativeBuildInputs = [
    autoreconfHook autoconf-archive pkg-config swig
    # configure (AX_PYTHON_DEVEL) still imports distutils; setuptools shims it.
    (buildPackages.python3.withPackages (p: [ p.setuptools ]))
    # SAI/meta generates saimetadata.c from the headers at build time.
    (buildPackages.perl.withPackages (p: [ p.XMLSimple ])) doxygen graphviz which
  ];
  buildInputs = [ sonic.swss-common sonic.libnl-sonic hiredis zeromq boost nlohmann_json libuuid zlib python3 ];

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
  '';

  configureFlags = [
    "--disable-static"
    "--disable-python2"
    "--with-sai=vs"
    "--with-swss-common-inc=${sonic.swss-common}/include/swss"
    "--with-swss-common-lib=${sonic.swss-common}/lib"
  ];
  env.NIX_CFLAGS_COMPILE = "-Wno-error -I${python3}/include/${python3.libPrefix} -I${sonic.swss-common}/include -I${lib.getDev sonic.libnl-sonic}/include/libnl3";
  # Several headers use uint32_t & co. without <cstdint> (fine with older libstdc++).
  env.CXXFLAGS = "-g -O2 -include cstdint -include sstream";
  makeFlags = [ "PYTHON3_BLDLIBRARY=-l${python3.libPrefix}" ];
  enableParallelBuilding = true;

  postInstall = ''
    mkdir -p $out/include/sai
    cp SAI/inc/*.h SAI/experimental/*.h SAI/meta/saimetadata*.h SAI/meta/saiserialize.h $out/include/sai/
  '';

  meta = {
    description = "SONiC SAI redis bridge (sairedis, syncd, virtual-switch SAI)";
    homepage = "https://github.com/sonic-net/sonic-sairedis";
    license = lib.licenses.asl20;
    platforms = lib.platforms.linux;
  };
}
