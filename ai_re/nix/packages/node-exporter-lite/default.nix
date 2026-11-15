# node-exporter-lite: node_exporter-compatible metrics in stdlib Python, for
# targets the Go toolchain cannot run on (see node_exporter_lite.py).
{ writeShellApplication, writeText, python3 }:
writeShellApplication {
  name = "node-exporter-lite";
  runtimeInputs = [ python3 ];
  text = ''exec python3 ${writeText "node_exporter_lite.py" (builtins.readFile ./node_exporter_lite.py)} "$@"'';
}
