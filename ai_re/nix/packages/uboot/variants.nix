# Which scopes and patches each uboot-* flake output is built from, as text:
#   nix eval --raw --impure --expr 'import ./nix/packages/uboot/variants.nix { flake = builtins.getFlake "path:'$PWD'/nix"; }'
# Read off the derivations (their patches attribute), so it cannot drift from
# what is built.
{ flake, system ? "x86_64-linux" }:
let
  series = import ./series.nix;
  scopeOf = builtins.listToAttrs (map (s: { name = s.file; value = s.scope; }) series);
  # "/nix/store/<32 chars>-0001-....patch" -> "0001-....patch"
  fileOf = p: builtins.substring 44 (-1) (builtins.unsafeDiscardStringContext (toString p));
  uniq = builtins.foldl' (acc: x: if builtins.elem x acc then acc else acc ++ [ x ]) [ ];
  pkgs = flake.packages.${system};
  names = builtins.filter (n: builtins.substring 0 6 n == "uboot-") (builtins.attrNames pkgs);
  filesOf = n: map fileOf (pkgs.${n}.patches or [ ]);
  patched = builtins.filter (n: filesOf n != [ ]) names;
  line = n:
    "${n}\n  scopes:  ${toString (uniq (map (f: scopeOf.${f} or "(not in series.nix: ${f})") (filesOf n)))}\n"
    + "  patches: ${toString (map (builtins.substring 0 4) (filesOf n))}\n";
in builtins.concatStringsSep "" (map line patched)
  + "no patches (stock builds, wrappers, tests): ${toString (builtins.filter (n: !builtins.elem n patched) names)}\n"
