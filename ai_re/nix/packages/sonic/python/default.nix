# SONiC's Python side as Nix packages: what the config loader, YANG validation
# and the show/config CLI need. One Python for all of them (python3 of pkgs).
#   py = sonic.python;  py.utilities  py.config-engine  py.yang-mgmt ...
# Upstream installs these as wheels with setup.py version checks via
# pkg_resources; here each is copied into site-packages (format "other"),
# dependencies are nixpkgs Python packages. Deviations, all here:
#  - libyang: nixpkgs' 3.13.6 instead of SONiC's 3.12.2 + PR 2362
#    (LYD_VALIDATE_NOEXTDEPS); nothing in the Python stack passes that flag.
#  - sonic-py-common without sonic_grpc (gNOI stubs; needs grpcio-tools).
#  - sonic-utilities: see ./utilities.nix.
{ lib, python3, libyang, iproute2, sonic }:
let
  py = python3.pkgs;
  bi = sonic.sources.buildimage;
  site = python3.sitePackages;
  # Copy python modules/packages into site-packages; wrap listed scripts.
  copyPkg = { pname, src, modules, scripts ? [ ], deps ? [ ], postPatch ? "", patches ? [ ]
            , extraInstall ? "", imports ? [ ], passthru ? { } }:
    py.buildPythonPackage {
      inherit pname src postPatch patches passthru;
      version = sonic.version "buildimage";
      format = "other";
      dontBuild = true;
      propagatedBuildInputs = deps;
      installPhase = ''
        runHook preInstall
        mkdir -p $out/${site} $out/bin
        for m in ${lib.escapeShellArgs modules}; do cp -r $m $out/${site}/; done
        for s in ${lib.escapeShellArgs scripts}; do install -m755 $s $out/bin/; done
        ${extraInstall}
        runHook postInstall
      '';
      pythonImportsCheck = imports;
      meta = { homepage = "https://github.com/sonic-net"; platforms = lib.platforms.linux; };
    };
  # natsort without its optional PyICU/fastnumbers (icu4c is 41 MiB on the
  # switch; plain natural sorting is all SONiC uses).
  natsort = py.natsort.overridePythonAttrs (_: { propagatedBuildInputs = [ ]; doCheck = false; });
  # swss-common's site-packages (swsscommon) as a Python dependency.
  swsscommon = py.toPythonModule sonic.swss-common;
  self = {
    inherit python3 copyPkg swsscommon natsort;

    # CESNET libyang-python at the tag SONiC builds (src/libyang3-py3/Makefile)
    # with SONiC's patches 0002-0004 (0001 is Debian packaging).
    libyang = py.buildPythonPackage {
      pname = "libyang";
      version = "3.1.0";
      src = sonic.sources.libyang-python;
      pyproject = true;
      # cffi's build step runs on the build machine (cross builds).
      build-system = with python3.pythonOnBuildForHost.pkgs; [ setuptools cffi ];
      dependencies = [ py.cffi ];
      buildInputs = [ libyang ];
      postPatch = ''
        for p in 0002-pr134-json-string-datatypes 0003-pr132-backlinks 0004-pr141-test-crash; do
          patch -p1 < ${bi}/src/libyang3-py3/patch/$p.patch
        done
      '';
      env = {
        LIBYANG_HEADERS = "${lib.getDev libyang}/include";
        LIBYANG_LIBRARIES = "${lib.getLib libyang}/lib";
      };
      pythonImportsCheck = [ "libyang" ];
      meta = { homepage = "https://github.com/CESNET/libyang-python"; license = lib.licenses.mit; };
    };

    py-common = copyPkg {
      pname = "sonic-py-common";
      src = "${bi}/src/sonic-py-common";
      modules = [ "sonic_py_common" ];
      deps = [ natsort py.pyyaml swsscommon ];
      postPatch = ''
        substituteInPlace sonic_py_common/multi_asic.py --replace-fail "'/bin/ip'" "'${iproute2}/bin/ip'"
      '';
      imports = [ "sonic_py_common.device_info" "sonic_py_common.multi_asic" "sonic_py_common.interface" ];
    };

    # YANG models with the templates rendered for the Python validator (as setup.py does).
    yang-models = let
      pyJinja = python3.pythonOnBuildForHost.withPackages (p: [ p.jinja2 ]);
    in py.buildPythonPackage {
      pname = "sonic-yang-models";
      version = sonic.version "buildimage";
      src = "${bi}/src/sonic-yang-models";
      format = "other";
      buildPhase = ''
        ${pyJinja}/bin/python3 -I - <<'EOF'
        import glob, os, jinja2
        env = jinja2.Environment(loader=jinja2.FileSystemLoader('yang-templates'), trim_blocks=True)
        for f in glob.glob('yang-templates/*.yang.j2'):
            b = os.path.basename(f)
            open('yang-models/' + b[:-3], 'w').write(env.get_template(b).render(yang_model_type="py"))
        EOF
      '';
      installPhase = ''
        mkdir -p $out/share/sonic
        cp -r yang-models $out/share/sonic/yang-models
      '';
      meta = { homepage = "https://github.com/sonic-net/sonic-buildimage"; license = lib.licenses.asl20; };
    };

    yang-mgmt = copyPkg {
      pname = "sonic-yang-mgmt";
      src = "${bi}/src/sonic-yang-mgmt";
      modules = [ "sonic_yang.py" "sonic_yang_ext.py" "sonic_yang_path.py" ];
      deps = [ self.libyang py.ijson py.jsonpointer py.jsondiff py.tabulate py.xmltodict ];
      imports = [ "sonic_yang" ];
    };

    config-engine = copyPkg {
      pname = "sonic-config-engine";
      src = "${bi}/src/sonic-config-engine";
      modules = [ "portconfig.py" "minigraph.py" "minigraph_ext.py" "openconfig_acl.py"
                  "config_samples.py" "asic_sensors_config.py" "smartswitch_config.py"
                  "sonic_yang_cfg_generator.py" ];
      scripts = [ "sonic-cfggen" ];
      deps = [ self.py-common self.yang-mgmt swsscommon py.lxml py.jinja2 py.pyyaml
               py.netaddr py.bitarray natsort ];
      # sonic-cfggen's templates (/usr/share/sonic/templates on SONiC).
      # Unwrapped copy for sonic-utilities' load_module_from_source.
      extraInstall = ''
        mkdir -p $out/share/sonic && cp -r data $out/share/sonic/templates
        install -Dm644 sonic-cfggen $out/lib/sonic-cfggen/sonic-cfggen
      '';
      imports = [ "portconfig" "minigraph" ];
    };

    platform-common = copyPkg {
      pname = "sonic-platform-common";
      src = sonic.sources.platform-common;
      modules = [ "sonic_platform_base" "sonic_platform_api" "sonic_sfp" "sonic_eeprom" ];
      deps = [ self.py-common self.config-engine natsort py.pyyaml ];
      imports = [ "sonic_platform_base.sonic_sfp.sfputilhelper" ];
    };

    utilities = import ./utilities.nix { inherit lib py python3 sonic; spy = self; };
  };
in self
