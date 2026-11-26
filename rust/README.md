# rust/

Rust tools for this monorepo, as one Cargo workspace.

## Conventions

- **One crate per tool** under `crates/<name>/`. Shared code becomes its own
  crate in `crates/` when a second tool needs it (not before).
- Each crate carries its own Nix next to it in `crates/<name>/nix/`:
  `package.nix` (a `callPackage`-able function) and, for services, `module.nix`
  (a NixOS module). Other trees (`nix/`, `ai_re/nix/`) import those files rather
  than copying them.
- `Cargo.lock` is committed: Nix builds from it (`cargoLock.lockFile`), so it is
  the single source of dependency versions.
- Keep dependencies few, pure Rust and without async runtimes or C
  libraries, so tools cross-compile to small embedded targets (ppc64 BE, arm)
  and closures stay tiny. Shared versions live in `[workspace.dependencies]`.
- `cargo fmt` (see `rustfmt.toml`) and `cargo clippy --all-targets` must be clean.

## Build and test

With a Rust toolchain on PATH, or through the nixpkgs the targets use:

```sh
cd rust
nix shell path:$(nix eval --impure --raw --expr '(builtins.getFlake "path:'$PWD'/../ai_re/nix").inputs.nixpkgs.outPath')#{cargo,rustc,clippy,rustfmt}
cargo build
cargo test
cargo fmt --all --check
cargo clippy --all-targets
```

## How Nix consumes it

`crates/<name>/nix/package.nix` builds with `rustPlatform.buildRustPackage`
and `cargoLock.lockFile = ../../../Cargo.lock`. Its source is the workspace
filtered with `lib.fileset` to `Cargo.toml`, `Cargo.lock` and the `.rs` /
`Cargo.toml` / `.yaml` files under `crates/`, so unrelated repository changes
(and edits to READMEs or the `.nix` files) do not cause rebuilds. Every crate
of the workspace is in that source, because Cargo needs all members present.

```nix
fand = pkgs.callPackage ./rust/crates/fand/nix/package.nix { };
imports = [ ./rust/crates/fand/nix/module.nix ];
```

Native and cross builds use the same function; tests run in the build only
when the build machine can execute the result.

## Crates

| crate | what |
|---|---|
| [fand](crates/fand/) | vendor-neutral fan control daemon for switches and appliances |

## Other workspaces

Their own Cargo workspace and lock file each, so that they do not rebuild fand:

| directory | what |
|---|---|
| [consoled](consoled/) | serial console server (consoled + line) |
| [sai](sai/) | a SAI (`libsai.so`) in Rust: safe core, virtual backend, small unsafe C-ABI rim; first slice |
| [cs8210](cs8210/) | CS8210-32X platform: the I2C FPGA's owner (cages, CMIS bring-up, optics for SONiC and Prometheus), LED tool |
| [lab-power](lab-power/) | the lab's power feeds (Home Assistant plugs, NetBotz 250 outlet), the `lab-power` CLI of the ai_re devshell ([usage](../ai_re/tools/README.md)) |
