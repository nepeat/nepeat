# libdashapi: C++ protobuf bindings for SONiC's DASH tables (orchagent links
# it unconditionally). Upstream recipe: its Makefile (target dashapi.so +
# "install"), driven by debian/rules.
# protobuf 3.21 (protobuf_21), as in Debian trixie: current protobuf needs
# abseil, whose headers require C++20; SONiC's code is C++14/17.
# Deviations: C++17 instead of -std=c++14 (nixpkgs' gtest needs it; same as
# swss); flags from pkg-config; the Python/SWIG bindings and dash_api_utils
# are not built (nothing in swss needs them).
{ lib, stdenv, pkg-config, protobuf_21, boost, gtest, sonic }:
let protobuf = protobuf_21; in
stdenv.mkDerivation {
  pname = "sonic-dash-api";
  version = sonic.version "dash-api";
  src = sonic.sources.dash-api;

  nativeBuildInputs = [ pkg-config protobuf ];   # protoc runs on the build machine
  buildInputs = [ protobuf ];
  passthru = { inherit protobuf; };
  checkInputs = [ boost gtest ];

  buildPhase = ''
    runHook preBuild
    mkdir -p build
    protoc -I=proto --cpp_out=build --experimental_allow_proto3_optional proto/*.proto
    $CXX -std=c++17 -g -O2 -fPIC -shared -Wl,-soname,libdashapi.so -Ibuild -o build/libdashapi.so \
      build/*.pb.cc misc/*.cpp $(pkg-config --cflags --libs protobuf)
    runHook postBuild
  '';

  # Upstream's "test" target, C++ half (the pytest half needs the Python bindings).
  doCheck = stdenv.buildPlatform.canExecute stdenv.hostPlatform;
  checkPhase = ''
    runHook preCheck
    $CXX -std=c++17 -DPROTO_PATH='"proto"' -Ibuild -Imisc misc/tests/*.cpp -Lbuild -ldashapi \
      $(pkg-config --cflags --libs protobuf) -lgtest -lpthread -lboost_filesystem -o build/test
    LD_LIBRARY_PATH=build build/test
    runHook postCheck
  '';

  installPhase = ''
    runHook preInstall
    install -Dm755 build/libdashapi.so -t $out/lib
    install -Dm644 build/*.pb.h misc/*.h -t $out/include/dash_api
    runHook postInstall
  '';

  meta = {
    description = "SONiC DASH API protobuf library";
    homepage = "https://github.com/sonic-net/sonic-dash-api";
    license = lib.licenses.asl20;
    platforms = lib.platforms.linux;
  };
}
