{
  lib,
  python3,
  src,
  fetchPypi,
  fetchFromGitHub,
  rustPlatform,
  cargo,
  rustc,
  cmake,
  buildNpmPackage,
}:

let
  rev = src.rev or null;
  srcDate = if src ? lastModifiedDate then lib.substring 0 8 src.lastModifiedDate else "unknown";

  # Version stamped into both the wheel and the Rust CLI.  Upstream derives it
  # from `git describe`, which a fetched tarball has no way to run, so it is
  # supplied through the documented OPENVIKING_VERSION /
  # SETUPTOOLS_SCM_PRETEND_VERSION_FOR_OPENVIKING escape hatches instead.
  #
  # Rather than hand-maintain a release number that silently goes stale, the
  # commit is carried as a PEP 440 local segment: `0.0.0+<sha>`.  It cannot
  # drift, and `ov --version` prints something you can `git checkout`.  A bare
  # `0.0.0.<sha>` would not work — the release segment must be numeric.
  pep440Version = if rev != null then "0.0.0+${rev}" else "0.0.0";

  # nixpkgs' convention for a VCS snapshot with no meaningful upstream tag.
  version = "0-unstable-${srcDate}";

  # Hashes of the vendored dependency trees.  These change whenever
  # openviking-src moves; `just update-openviking` rewrites both lines in
  # place, so keep them on one line each in `name = "sha256-...";` form.
  cargoHash = "sha256-v9bPp2QROTY7ekWeoJYeyf70QVf7rBjV8e9JsNPUf2o=";
  npmDepsHash = "sha256-Jn/au7BWkwDK4mEmi3xg+7etKMWjzr2KpPw47VPyoEo=";

  # One vendored cargo tree covers the whole workspace, so both ov_cli and
  # ragfs-python share it.
  cargoDeps = rustPlatform.fetchCargoVendor {
    inherit src;
    name = "openviking-cargo-deps-${version}";
    hash = cargoHash;
  };

  # The Rust CLI that `ov` and `openviking` exec into.
  ovCli = rustPlatform.buildRustPackage {
    pname = "ov-cli";
    inherit version src cargoDeps;

    buildAndTestSubdir = "crates/ov_cli";
    env.OPENVIKING_VERSION = pep440Version;

    # reqwest is built with rustls and default-features off, so there is no
    # openssl to wire up.
    doCheck = false;

    meta.mainProgram = "ov";
  };

  # The /studio SPA.  setup.py would run `npm ci` itself; building it here
  # keeps the install offline.
  webStudio = buildNpmPackage {
    pname = "openviking-web-studio";
    inherit version;

    src = "${src}/web-studio";
    inherit npmDepsHash;

    # Matches the `--base=/studio/` upstream passes.
    npmBuildFlags = [
      "--"
      "--base=/studio/"
    ];

    installPhase = ''
      runHook preInstall
      cp -r dist $out
      runHook postInstall
    '';
  };

  # A tree-sitter grammar binding, built the same way nixpkgs builds the
  # grammars it already carries.  The GitHub tarball is used rather than the
  # PyPI sdist because the sdists omit the vendored src/tree_sitter/parser.h
  # that src/parser.c includes.
  grammar =
    {
      pname,
      version,
      owner ? "tree-sitter",
      hash,
      module ? lib.replaceStrings [ "-" ] [ "_" ] pname,
    }:
    python.pkgs.buildPythonPackage {
      inherit pname version;
      pyproject = true;

      src = fetchFromGitHub {
        inherit owner hash;
        repo = pname;
        tag = "v${version}";
      };

      build-system = [ python.pkgs.setuptools ];
      optional-dependencies.core = [ python.pkgs.tree-sitter ];

      doCheck = false;
      pythonImportsCheck = [ module ];
    };

  python = python3.override {
    self = python;
    packageOverrides = final: prev: {
      # nixpkgs master: pandas-stubs' test suite trips
      # PytestRemovedIn10Warning under pytest 9, which breaks pdfplumber's
      # build.  Unrelated to openviking; drop when nixpkgs catches up.
      pandas-stubs = prev.pandas-stubs.overridePythonAttrs {
        doCheck = false;
        # pandas itself is only a check input, so its import check goes too.
        pythonImportsCheck = [ ];
      };

      openviking-sdk = final.buildPythonPackage rec {
        pname = "openviking-sdk";
        version = "0.1.5";
        pyproject = true;

        src = fetchPypi {
          pname = "openviking_sdk";
          inherit version;
          hash = "sha256-8BfeyTgmeq6mWRIhZfKHLhiZM6VgS6bZ7AroZ7VY7K8=";
        };

        build-system = with final; [
          setuptools
          setuptools-scm
        ];
        dependencies = [ final.httpx ];

        doCheck = false;
        pythonImportsCheck = [ "openviking_sdk" ];
      };

      volcengine = final.buildPythonPackage rec {
        pname = "volcengine";
        version = "1.0.227";
        pyproject = true;

        src = fetchPypi {
          inherit pname version;
          hash = "sha256-GqLSBJ67o3FB9JKkS+VphitEebsx/zdrZRIMUj96ilo=";
        };

        build-system = [ final.setuptools ];
        dependencies = with final; [
          google
          protobuf
          pycryptodome
          pytz
          requests
          retry
          six
          tenacity
        ];

        doCheck = false;
        pythonImportsCheck = [ "volcengine" ];
      };

      volcengine-python-sdk = final.buildPythonPackage rec {
        pname = "volcengine-python-sdk";
        version = "5.0.43";
        pyproject = true;

        src = fetchPypi {
          pname = "volcengine_python_sdk";
          inherit version;
          hash = "sha256-xM4wQuYRRNtSkk/4Aa6hWOTUIV0/99/NcmzGl4Ez+a8=";
        };

        build-system = [ final.setuptools ];
        dependencies = with final; [
          certifi
          python-dateutil
          six
          urllib3
        ];

        # openviking depends on the [ark] extra.
        optional-dependencies.ark = with final; [
          anyio
          cryptography
          httpx
          pydantic
        ];

        doCheck = false;
        pythonImportsCheck = [ "volcenginesdkcore" ];
      };

      # Version-locked to nixpkgs' opentelemetry-instrumentation.
      opentelemetry-instrumentation-asyncio = final.buildPythonPackage rec {
        pname = "opentelemetry-instrumentation-asyncio";
        inherit (final.opentelemetry-instrumentation) version;
        pyproject = true;

        src = fetchPypi {
          pname = "opentelemetry_instrumentation_asyncio";
          inherit version;
          hash = "sha256-bka9lxsJGqC6ZOvyNY6Ms7YFKi8SzqMMc8wNUn/8osE=";
        };

        build-system = [ final.hatchling ];
        dependencies = with final; [
          opentelemetry-api
          opentelemetry-instrumentation
          opentelemetry-semantic-conventions
          wrapt
        ];

        doCheck = false;
        pythonImportsCheck = [ "opentelemetry.instrumentation.asyncio" ];
      };

      # Grammars openviking imports directly that nixpkgs does not carry yet.
      tree-sitter-typescript = grammar {
        pname = "tree-sitter-typescript";
        version = "0.23.2";
        hash = "sha256-CU55+YoFJb6zWbJnbd38B7iEGkhukSVpBN7sli6GkGY=";
      };

      tree-sitter-java = grammar {
        pname = "tree-sitter-java";
        version = "0.23.5";
        hash = "sha256-OvEO1BLZLjP3jt4gar18kiXderksFKO0WFXDQqGLRIY=";
      };

      tree-sitter-cpp = grammar {
        pname = "tree-sitter-cpp";
        version = "0.23.4";
        hash = "sha256-tP5Tu747V8QMCEBYwOEmMQUm8OjojpJdlRmjcJTbe2k=";
      };

      tree-sitter-go = grammar {
        pname = "tree-sitter-go";
        version = "0.25.0";
        hash = "sha256-y7bTET8ypPczPnMVlCaiZuswcA7vFrDOc2jlbfVk5Sk=";
      };

      tree-sitter-php = grammar {
        pname = "tree-sitter-php";
        version = "0.24.1";
        hash = "sha256-SUoWvPnNpgg5QMxbTw7XfXEoxyOkqnNFPnrMZnoiJH0=";
      };

      tree-sitter-lua = grammar {
        pname = "tree-sitter-lua";
        version = "0.5.0";
        owner = "tree-sitter-grammars";
        hash = "sha256-VzaaN5pj7jMAb/u1fyyH6XmLI+yJpsTlkwpLReTlFNY=";
      };

      # The Rust AGFS binding.  openviking.pyagfs prefers an importable
      # `ragfs_python` over the copy setup.py would otherwise vendor into
      # openviking/lib/, so providing it as a normal dependency is enough.
      ragfs-python = final.buildPythonPackage {
        pname = "ragfs-python";
        version = "0.1.0";
        pyproject = true;

        inherit src cargoDeps;
        buildAndTestSubdir = "crates/ragfs-python";

        nativeBuildInputs = [
          rustPlatform.cargoSetupHook
          rustPlatform.maturinBuildHook
          cargo
          rustc
        ];

        doCheck = false;
        # The #[pymodule] initialiser imports openviking.storage.errors, so
        # this extension is only importable with openviking already on the
        # path — it cannot be import-checked on its own.
        pythonImportsCheck = [ ];
      };
    };
  };
in
python.pkgs.buildPythonApplication {
  pname = "openviking";
  inherit version src;
  pyproject = true;

  build-system = with python.pkgs; [
    setuptools
    setuptools-scm
    wheel
  ];

  # setup.py drives cmake itself for the vectordb engine, so the generic cmake
  # configure hook must stay out of the way.
  nativeBuildInputs = [ cmake ];
  dontUseCmakeConfigure = true;

  # cmake comes from nativeBuildInputs rather than pip, and maturin is only
  # needed for the ragfs-python build we skip (see OV_SKIP_RAGFS_BUILD).
  postPatch = ''
    substituteInPlace pyproject.toml \
      --replace-fail '"cmake>=3.15",' "" \
      --replace-fail '"maturin>=1.0,<2.0",' ""
  '';

  env = {
    # `git describe` is unavailable in a tarball checkout.
    OPENVIKING_VERSION = pep440Version;
    SETUPTOOLS_SCM_PRETEND_VERSION_FOR_OPENVIKING = pep440Version;

    # Reuse the separately-built Rust CLI instead of invoking cargo here.
    OV_PREBUILT_BIN_DIR = "${ovCli}/bin";

    # ragfs_python ships as a real Python dependency (see the override above)
    # rather than a vendored .so, so setup.py must not demand the bundled copy.
    OV_SKIP_RAGFS_BUILD = "1";
    OV_REQUIRE_RAGFS_BUILD = "0";
  };

  # setup.py's _build_web_studio() short-circuits when the bundle is already
  # present, which keeps `npm ci` from running inside the sandbox.
  preBuild = ''
    mkdir -p openviking/web_studio
    cp -r ${webStudio} openviking/web_studio/dist
    chmod -R u+w openviking/web_studio
  '';

  dependencies =
    with python.pkgs;
    [
      openviking-sdk
      volcengine
      apscheduler
      argon2-cffi
      charset-normalizer
      cryptography
      defusedxml
      ebooklib
      fastapi
      feedparser
      grep-ast
      httpx
      jinja2
      json-repair
      lark-oapi
      litellm
      loguru
      mcp
      olefile
      openai
      openpyxl
      pathspec
      pdfminer-six
      pdfplumber
      protobuf
      pydantic
      python-docx
      python-multipart
      python-pptx
      pyyaml
      requests
      scrapy
      tabulate
      trafilatura
      tree-sitter
      tree-sitter-language-pack
      typer
      typing-extensions
      urllib3
      uvicorn
      xlrd
      xxhash
      # OpenTelemetry
      opentelemetry-api
      opentelemetry-sdk
      opentelemetry-exporter-otlp-proto-grpc
      opentelemetry-exporter-otlp-proto-http
      opentelemetry-instrumentation-asyncio
      # tree-sitter grammars imported by openviking.parse
      tree-sitter-c-sharp
      tree-sitter-cpp
      tree-sitter-go
      tree-sitter-java
      tree-sitter-javascript
      tree-sitter-lua
      tree-sitter-php
      tree-sitter-python
      tree-sitter-rust
      tree-sitter-typescript
      # Rust AGFS binding, built from crates/ragfs-python
      ragfs-python
    ]
    ++ volcengine-python-sdk.optional-dependencies.ark
    ++ [ volcengine-python-sdk ];

  # nixpkgs carries tree-sitter-language-pack 1.4.1; openviking asks for >=1.12
  # but never imports it directly (only grep-ast does, against the stable API).
  pythonRelaxDeps = [ "tree-sitter-language-pack" ];

  postInstall = ''
    # `vikingbot` needs the huge [bot] extra, which we do not package.
    rm -f $out/bin/vikingbot

    # setup.py copies the CLI in via OV_PREBUILT_BIN_DIR; keep it executable.
    chmod +x $out/${python.sitePackages}/openviking/bin/ov
  '';

  pythonImportsCheck = [
    "openviking"
    "openviking_cli"
  ];

  passthru = {
    inherit ovCli webStudio;
    inherit (python.pkgs) ragfs-python;
  };

  meta = {
    description = "Agent-native context database";
    homepage = "https://github.com/volcengine/OpenViking";
    license = lib.licenses.agpl3Only;
    mainProgram = "ov";
    platforms = lib.platforms.unix;
  };
}
