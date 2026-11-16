# LD_PRELOAD heap profiler for leak hunting on the switch (malloc-trace.c has
# the usage). Built like everything else for the target:
#   nix build path:./nix#... sonic.routing.malloc-trace (see devices/sonic-nixos/bfd.md)
{ stdenv }:
stdenv.mkDerivation {
  pname = "malloc-trace";
  version = "0.1";
  src = ./malloc-trace.c;
  dontUnpack = true;
  buildPhase = "$CC -O2 -g -fPIC -shared -Wall -o libmalloc-trace.so $src -ldl -lpthread";
  installPhase = "install -Dm755 libmalloc-trace.so $out/lib/libmalloc-trace.so";
}
