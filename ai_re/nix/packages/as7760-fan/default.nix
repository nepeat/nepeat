{ pkgs, kernel }:
# AS7760-32X fan CPLD hwmon driver (out-of-tree Rust module for the given kernel,
# which needs CONFIG_RUST: ./rust.nix) and its userspace control/monitor tool.
{
  driver = pkgs.callPackage ./rust.nix { inherit kernel; };
  tool = pkgs.writeShellApplication {
    name = "as7760-fan";
    runtimeInputs = [ pkgs.python3 ];
    text = ''exec python3 ${pkgs.writeText "as7760-fan-control.py" (builtins.readFile ./control.py)} "$@"'';
  };
}
