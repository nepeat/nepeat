# The oracle (README.md): the libifcs call tracer for one IFCS ABI and the
# SAI script runner for one libsai, plus oracle.py and the scripts.
#   ours (arm64, IFCS 0.12.41 via our backend's table):
#     cross.callPackage ./oracle { saiInc = sai.saiHeaders; saiLib = sai; abi = "ifcs-0.12.41"; }
#   isai (amd64, IFCS 0.16.2 inside isai's libsai, LD_PRELOAD under qemu-ipd):
#     pkgs.callPackage ./oracle { saiInc = "<isai>/include/sai"; saiLib = isai; abi = "isai-6.2.0";
#                                 ctypes = "<isai>/share/isai/pyctypes/ifcs_ctypes.py"; }
# The isai variant is generated from the vendor ctypes file at build time
# (names, signatures, layouts only), so it is a store-only build product.
{ lib, stdenv, buildPackages, nlohmann_json, saiInc, saiLib, abi, ctypes ? null }:
stdenv.mkDerivation {
  pname = "ifcs-oracle-${abi}";
  version = "1";
  src = lib.fileset.toSource {
    root = ./..;
    # what the build reads (the drivers and docs do not rebuild it)
    fileset = lib.fileset.unions [ ./gen_tracer.py ./trace_rt.c ./trace_rt.h ./sai_script.cpp ./oracle.py ./scripts
                                   ../src/ifcs_api.h ../tools/trap_frames.c ];
  };
  nativeBuildInputs = [ buildPackages.python3 ];
  dontConfigure = true;
  buildPhase = ''
    runHook preBuild
    cd oracle
    ${if ctypes == null then ''
      python3 -I gen_tracer.py --header ../src/ifcs_api.h --abi ${abi} -o trace_gen.c
    '' else ''
      # every ifcs_* function isai's libsai imports (PLT / GOT): the interposable calls
      ''${READELF:-readelf} -W -r ${saiLib}/lib/libsai.so.1.0 \
        | awk '$3 ~ /JUMP_SLOT|GLOB_DAT/ && $5 ~ /^ifcs_/ {print $5}' | sort -u > imports.txt
      python3 -I gen_tracer.py --ctypes ${ctypes} --imports imports.txt --abi ${abi} -o trace_gen.c
    ''}
    $CC -O2 -fPIC -shared -Wall -I../src -I. -Wl,-soname,libifcs-trace.so -o libifcs-trace.so trace_gen.c trace_rt.c -ldl -lpthread
    $CXX -O2 -std=c++17 -Wall -I${saiInc} -I${saiInc}/../experimental -I${nlohmann_json}/include \
      -o sai-script sai_script.cpp -L${saiLib}/lib -lsai -Wl,-rpath,${saiLib}/lib -ldl -lpthread
    $CC -O2 -Wall -o trap-frames ../tools/trap_frames.c -lpthread
    runHook postBuild
  '';
  installPhase = ''
    install -Dm755 libifcs-trace.so $out/lib/libifcs-trace.so
    install -Dm755 sai-script $out/bin/sai-script
    install -Dm755 trap-frames $out/bin/trap-frames
    install -Dm644 trace_gen.c $out/share/ifcs-oracle/trace_gen.c
    install -Dm755 oracle.py $out/share/ifcs-oracle/oracle.py
    cp -r scripts $out/share/ifcs-oracle/scripts
  '';
  meta = {
    description = "libifcs call tracer + SAI script runner (${abi})";
    platforms = lib.platforms.linux;
  } // lib.optionalAttrs (ctypes != null) { license = lib.licenses.unfree; };   # generated from the vendor ctypes file
}
