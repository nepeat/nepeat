# The AS7760-32X sonic_platform (SONiC platform API 2.0, ./sonic_platform) as
# a Python package, with as7760-qsfp's qsfp.py as the module as7760_qsfp (its
# I2C paths: bus lookup, CPLD#1, combined transfers).
#   pkgs.callPackage ./sonic-platform-package.nix { sonic = import ../../../packages/sonic { inherit pkgs; }; }
# passthru.tests.unit: ./sonic_platform_test.py on the fake board
# (./fake_board.py); build host only.
{ lib, runCommand, sonic }:
let
  spy = sonic.python;
  py = spy.python3.pkgs;
  src = builtins.path {
    path = ./sonic_platform;
    name = "as7760-sonic-platform";
    filter = p: _: !(lib.hasSuffix "__pycache__" p) && !(lib.hasSuffix ".pyc" p);
  };
  qsfp = builtins.path { path = ../../../packages/as7760-qsfp/qsfp.py; name = "as7760_qsfp.py"; };
  site = spy.python3.sitePackages;
  self = py.buildPythonPackage {
    pname = "sonic-platform-accton-as7760-32x";
    version = "0.1";
    inherit src;
    format = "other";
    dontUnpack = true;
    dontBuild = true;
    propagatedBuildInputs = [ spy.platform-common spy.py-common ];
    installPhase = ''
      mkdir -p $out/${site}
      cp -r ${src} $out/${site}/sonic_platform
      chmod -R u+w $out/${site}/sonic_platform
      cp ${qsfp} $out/${site}/as7760_qsfp.py
    '';
    pythonImportsCheck = [ "sonic_platform.platform" "as7760_qsfp" ];
    passthru.tests.unit = runCommand "as7760-sonic-platform-test" {
      nativeBuildInputs = [ (spy.python3.withPackages (_: [ self ])) ];
    } ''
      mkdir t && cp ${./fake_board.py} t/fake_board.py && cp ${./sonic_platform_test.py} t/sonic_platform_test.py
      cd t && python3 sonic_platform_test.py 2>&1 | tee $out
      grep -q '^OK' $out
    '';
  };
in self
