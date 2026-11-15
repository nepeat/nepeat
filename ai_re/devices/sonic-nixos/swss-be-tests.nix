# sonic-swss unit tests cross-built for the AS7760 (ppc64 BE), to run under
# qemu-ppc64 -cpu e6500 (results: swss-build-20261115.txt). Build:
#   nix build --impure -f devices/sonic-nixos/swss-be-tests.nix
# then copy result/be-tests somewhere writable and run each gtest binary from
# its own directory (tests/mock_tests/tests* read ./database_config.json).
let
  p = (builtins.getFlake "path:${toString ../../nix}").nixosConfigurations.as7760.pkgs;
  sonic = import ../../nix/packages/sonic { pkgs = p; };
in (sonic.swss.override { doCheck = true; }).overrideAttrs (o: {
  pname = "sonic-swss-be-tests";
  postInstall = o.postInstall + ''
    mkdir -p $out/be-tests/orchagent/p4orch
    cp -r tests $out/be-tests/
    cp -r orchagent/p4orch/tests $out/be-tests/orchagent/p4orch/
    find $out/be-tests -name '*.o' -delete
  '';
})
