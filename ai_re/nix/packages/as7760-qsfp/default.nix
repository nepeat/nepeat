# as7760-qsfp: read and decode front-panel module EEPROMs (SFF-8636 / SFF-8472).
{ writeShellApplication, writeText, python3 }:
writeShellApplication {
  name = "as7760-qsfp";
  runtimeInputs = [ python3 ];
  text = ''exec python3 ${writeText "as7760-qsfp.py" (builtins.readFile ./qsfp.py)} "$@"'';
}
