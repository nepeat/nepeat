# The AS7760-32X sonic_platform (SONiC platform API 2.0, ./sonic_platform) as
# a Python package, with as7760-qsfp's qsfp.py as the module as7760_qsfp (its
# I2C paths: bus lookup, CPLD#1, combined transfers).
#   pkgs.callPackage ./sonic-platform-package.nix { sonic = import ../../../packages/sonic { inherit pkgs; }; }
# passthru.tests.unit: ./sonic_platform_test.py on the fake board
# (./fake_board.py); build host only.
# thermals: a derivative board's sensor list (sonic_platform/thermal.py:
# [ name hwmon-name i2c-address temp-input high critical ]), null = the
# AS7760's four LM75s; unitTest: the test run on the fake board.
{ lib, runCommand, writeText, sonic, pname ? "sonic-platform-accton-as7760-32x", thermals ? null
, unitTest ? ./sonic_platform_test.py }:
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
    inherit pname;
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
    '' + lib.optionalString (thermals != null) ''
      cp ${writeText "sensors.json" (builtins.toJSON thermals)} $out/${site}/sonic_platform/sensors.json
    '';
    pythonImportsCheck = [ "sonic_platform.platform" "as7760_qsfp" ];
    passthru.tests.unit = runCommand "${lib.removeSuffix "-32x" (lib.removePrefix "sonic-platform-accton-" pname)}-sonic-platform-test" {
      nativeBuildInputs = [ (spy.python3.withPackages (_: [ self ])) ];
    } ''
      mkdir t && cp ${./fake_board.py} t/fake_board.py && cp ${unitTest} t/sonic_platform_test.py
      cd t && python3 sonic_platform_test.py 2>&1 | tee $out
      grep -q '^OK' $out
    '';
  };
in self
