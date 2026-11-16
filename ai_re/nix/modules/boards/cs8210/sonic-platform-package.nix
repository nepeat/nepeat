# The CS8210-32X sonic_platform (SONiC platform API 2.0, ./sonic_platform) as
# a Python package. Standard library only on top of sonic-platform-common
# (I2C via i2c-dev ioctls, the I2C FPGA via mmap of its BAR0).
#   pkgs.callPackage ./sonic-platform-package.nix { sonic = import ../../../packages/sonic { inherit pkgs; }; }
# passthru.tests.unit: ./sonic_platform_test.py on the fake board
# (./fake_board.py; sonic_platform/fake.py is the fake hardware); build host only.
{ lib, runCommand, sonic }:
let
  spy = sonic.python;
  py = spy.python3.pkgs;
  src = builtins.path {
    path = ./sonic_platform;
    name = "cs8210-sonic-platform";
    filter = p: _: !(lib.hasSuffix "__pycache__" p) && !(lib.hasSuffix ".pyc" p);
  };
  site = spy.python3.sitePackages;
  self = py.buildPythonPackage {
    pname = "sonic-platform-celestica-cs8210-32x";
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
    '';
    pythonImportsCheck = [ "sonic_platform.platform" ];
    passthru.tests.unit = runCommand "cs8210-sonic-platform-test" {
      nativeBuildInputs = [ (spy.python3.withPackages (_: [ self ])) ];
    } ''
      mkdir t && cp ${./fake_board.py} t/fake_board.py && cp ${./sonic_platform_test.py} t/sonic_platform_test.py
      cd t && python3 -I sonic_platform_test.py 2>&1 | tee $out
      grep -q '^OK' $out
    '';
  };
in self
