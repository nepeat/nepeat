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
  pkg-config,
  zstd,
  openssl,
  sqlite,
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
  cargoHash = "sha256-uApNeWoPSZ35r7YGDq6KO7btOSeuV9ZXc0q5tJeEvOA=";
  npmDepsHash = "sha256-HI9MnfF2VoBkQqleXTI9mA5LFs9VZCaiv7ILvdTa/sY=";

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

    # zstd-sys probes libzstd through pkg-config.
    nativeBuildInputs = [ pkg-config ];
    buildInputs = [ zstd ];

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

    # CodeMirror's jsdom tests take a little over Vitest's 5s default on a
    # loaded Nix builder.  Keep the upstream prebuild test suite enabled, but
    # give those tests enough headroom to finish.
    postPatch = ''
      substituteInPlace vite.config.ts \
        --replace-fail "test: {" "test: { testTimeout: 15000,"
    '';

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

  # OpenViking requires >=2.9.1; nixpkgs still carries 2.9.0. Keep this local
  # to avoid rebuilding unrelated packages that use CairoSVG in their tests.
  cairosvg = python.pkgs.cairosvg.overridePythonAttrs (old: rec {
    version = "2.9.1";
    src = fetchFromGitHub {
      owner = "Kozea";
      repo = "CairoSVG";
      tag = version;
      hash = "sha256-yIUuhBmq15xwHznEq8l5z8QSkhQQc3GjTbJw5L2ZX+I=";
    };
    meta = old.meta // {
      changelog = "https://github.com/Kozea/CairoSVG/releases/tag/${version}";
    };
  });

  python = python3.override {
    self = python;
    packageOverrides = final: prev: {
      # OpenViking's process engine uses PackConfig and process(), which are
      # absent from nixpkgs' older Python implementation.
      tree-sitter-language-pack = final.buildPythonPackage rec {
        pname = "tree-sitter-language-pack";
        version = "1.13.3";
        pyproject = true;

        # The PyPI sdist omits workspace crates from its Cargo.lock.
        src = fetchFromGitHub {
          owner = "xberg-io";
          repo = "tree-sitter-language-pack";
          tag = "v${version}";
          hash = "sha256-1IhLsN1HgkSnYPaK9RaFRZbXn9DSzx5hGnyVSV03E6M=";
        };

        buildAndTestSubdir = "packages/python";
        cargoDeps = rustPlatform.fetchCargoVendor {
          inherit src;
          hash = "sha256-dwtJ9/dLrX15qeRq14Nr0exEzqum3toWy9Enppbmtuo=";
        };

        nativeBuildInputs = [
          rustPlatform.cargoSetupHook
          rustPlatform.maturinBuildHook
          cargo
          rustc
          pkg-config
        ];
        buildInputs = [
          openssl
          zstd
        ];
        # Grammars load on demand at runtime, as in upstream's wheels.
        # Do not fetch parser sources inside the Nix build sandbox.
        env.TSLP_OFFLINE = "1";
        dependencies = [ final.tree-sitter ];
        doCheck = false;
        pythonImportsCheck = [ "tree_sitter_language_pack" ];
      };

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
        version = "0.1.17";
        pyproject = true;

        src = fetchPypi {
          pname = "openviking_sdk";
          inherit version;
          hash = "sha256-nzZeAqeArQTkVzGAkk5a++cfhDQpVjVq/B5oNLIpF1E=";
        };

        build-system = with final; [
          setuptools
          setuptools-scm
        ];
        dependencies = [ final.httpx ];

        doCheck = false;
        pythonImportsCheck = [ "openviking_sdk" ];
      };

      # OpenViking uses the MCP 1.x FastMCP/auth APIs; nixpkgs now carries 2.x.
      mcp = final.buildPythonPackage rec {
        pname = "mcp";
        version = "1.27.0";
        pyproject = true;

        src = fetchPypi {
          inherit pname version;
          hash = "sha256-09w1p+7A1FjB2kl2pI+YIJfdqrh+J4xVEdWkpW6FK4M=";
        };

        build-system = with final; [ hatchling uv-dynamic-versioning ];
        dependencies = with final; [
          anyio
          httpx
          httpx-sse
          jsonschema
          pydantic
          pydantic-settings
          pyjwt
          python-multipart
          sse-starlette
          starlette
          typing-extensions
          typing-inspection
          uvicorn
        ] ++ final.pyjwt.optional-dependencies.crypto;
        optional-dependencies = {
          cli = with final; [ python-dotenv typer ];
          rich = [ final.rich ];
          ws = [ final.websockets ];
        };

        doCheck = false;
        pythonImportsCheck = [ "mcp" "mcp.server.fastmcp" ];
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

      # Rust document->Markdown converter behind openviking's AnyDoc parser.
      # Installs as firecrawl-anydoc, imports as `anydoc`.
      firecrawl-anydoc = final.buildPythonPackage rec {
        pname = "firecrawl-anydoc";
        version = "0.2.4";
        pyproject = true;

        src = fetchPypi {
          pname = "firecrawl_anydoc";
          inherit version;
          hash = "sha256-PilGAnL+qBzeCP1a8R9rDx/wWRkhTdyTmGf3I2LIMDI=";
        };

        cargoDeps = rustPlatform.fetchCargoVendor {
          inherit src;
          name = "firecrawl-anydoc-cargo-deps-${version}";
          hash = "sha256-yhW5HSzVrZCms4x36J2NGIn1e5YLIgjuNKXM9EJ9J8c=";
        };

        nativeBuildInputs = [
          rustPlatform.cargoSetupHook
          rustPlatform.maturinBuildHook
          cargo
          rustc
        ];

        doCheck = false;
        pythonImportsCheck = [ "anydoc" ];
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
          pkg-config
        ];
        buildInputs = [
          openssl
          sqlite
        ];

        # The s3 feature drags in openssl-sys, which would otherwise vendor
        # OpenSSL from source (and need perl the sandbox lacks).
        env.OPENSSL_NO_VENDOR = "1";

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

  # The bundled leveldb/spdlog trees compile with their own warning flags that
  # drop -Wformat, which turns nixpkgs' injected -Werror=format-security into a
  # hard error on every translation unit.
  hardeningDisable = [ "format" ];

  # cmake comes from nativeBuildInputs rather than pip, and maturin is only
  # needed for the ragfs-python build we skip (see OV_SKIP_RAGFS_BUILD).
  postPatch = ''
    substituteInPlace pyproject.toml \
      --replace-fail '"cmake>=3.15",' "" \
      --replace-fail '"maturin>=1.0,<2.0",' ""
    substituteInPlace setup.py \
      --replace-fail 'CMAKE_OSX_DEPLOYMENT_TARGET=10.15' \
      'CMAKE_OSX_DEPLOYMENT_TARGET=11.0'
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
      firecrawl-anydoc
      openviking-sdk
      volcengine
      aiohttp
      apscheduler
      argon2-cffi
      async-timeout
      cairosvg
      charset-normalizer
      cryptography
      defusedxml
      ebooklib
      fastapi
      feedparser
      firecrawl-anydoc
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
      orjson
      packaging
      pathspec
      pdfminer-six
      pdfplumber
      prometheus-client
      protobuf
      pydantic
      python-docx
      python-multipart
      python-pptx
      pyyaml
      rapidfuzz
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

  # litellm is capped at <1.91.2 upstream while nixpkgs is on 1.97; openviking
  # only touches the stable surface (completion/embedding + their async twins,
  # drop_params, suppress_debug_info), so the cap is dropped rather than
  # pinning an older litellm.
  pythonRelaxDeps = [
    "litellm"
    # Allow nixpkgs' newer patch release of the C# grammar.
    "tree-sitter-c-sharp"
  ];

  postInstall = ''
    # `vikingbot` needs the huge [bot] extra, which we do not package.
    rm -f $out/bin/vikingbot

    # setup.py copies the CLI in via OV_PREBUILT_BIN_DIR; keep it executable.
    chmod +x $out/${python.sitePackages}/openviking/bin/ov
  '';

  pythonImportsCheck = [
    "openviking"
    "openviking_cli"
    "openviking.server.mcp_endpoint"
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
