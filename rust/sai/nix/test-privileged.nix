# The tests that make kernel netdevs (TAP devices, AF_PACKET wires), run as root in a NixOS VM:
# crates/tap-netdev/tests/privileged.rs and crates/sai-core/tests/netdev.rs. Everywhere else
# (cargo test, the Nix package's check phase, the test bundle on a switch) they skip, because
# only this VM sets SAI_RS_PRIVILEGED_TESTS=1.
#   import ./test-privileged.nix { pkgs = p; saiRs = <./package.nix for p>; }
{ pkgs, saiRs }:
let
  triple = pkgs.stdenv.hostPlatform.rust.rustcTarget;
  suites = [ "privileged" "netdev" ];
  tests = saiRs.overrideAttrs (old: {
    pname = "sai-rs-privileged-tests";
    doCheck = false;
    cargoBuildFlags = [ "-p" "tap-netdev" ];
    cargoCheckType = "debug";
    cargoTestFlags = [ "-p" "tap-netdev" "-p" "sai-core" "--no-run" ];
    preCheck = "";
    postBuild = ''
      cargoCheckHook
      cargoTestFlags="$cargoTestFlags --message-format=json" cargoCheckHook > cargo-messages.txt
      mkdir -p $out/tests
      grep '^{' cargo-messages.txt \
        | ${pkgs.buildPackages.jq}/bin/jq -r 'select(.reason == "compiler-artifact" and .profile.test and .executable != null) | .target.name + " " + .executable' \
        | while read -r name exe; do
            case " ${toString suites} " in *" $name "*) cp "$exe" $out/tests/$name ;; esac
          done
      for s in ${toString suites}; do test -x $out/tests/$s; done
    '';
    installPhase = "runHook preInstall; runHook postInstall";
    postInstall = "";
    meta = old.meta // { description = "sai-rs tests that create netdevs (run as root in a VM only)"; };
  });
in
pkgs.testers.runNixOSTest {
  name = "sai-rs-privileged";
  nodes.machine = { pkgs, ... }: {
    boot.kernelModules = [ "tun" ];
    environment.systemPackages = [ pkgs.iproute2 ];
  };
  testScript = ''
    machine.wait_for_unit("multi-user.target")
    for suite in [${pkgs.lib.concatMapStringsSep ", " (s: ''"${s}"'') suites}]:
        out = machine.succeed(f"SAI_RS_PRIVILEGED_TESTS=1 ${tests}/tests/{suite} --test-threads 1 2>&1")
        print(out)
        assert "skipped" not in out, suite
        assert "test result: ok" in out, suite
    # nothing left behind
    links = machine.succeed("ip -o link show")
    assert "sairs-" not in links, links
  '';
}
