{ modulesPath, ... }:
let
  # Pinned unstable's user module unconditionally requests four units omitted
  # by systemd when machined/nspawn/vmspawn are disabled. Mirror the guards
  # already used for the corresponding system units; keep every other option.
  # A nixpkgs that words these lines differently must fail here, not go on
  # with the replacement silently skipped.
  changed = what: old: new:
    if new == old then throw "profiles/systemd-minimal.nix: nixpkgs' systemd/user.nix changed (${what} not found); redo or drop this workaround" else new;
  original = builtins.readFile (modulesPath + "/system/boot/systemd/user.nix");
  pruned = changed "the four unit names" original (builtins.replaceStrings [
    "    \"machine.slice\"\n"
    "    \"machines.target\"\n"
    "    \"systemd-nspawn@.service\"\n"
    "    \"systemd-vmspawn@.service\"\n"
  ] [ "" "" "" "" ] original);
  fixed = changed "the withImportd list" pruned (builtins.replaceStrings [ "  ++ lib.optionals systemd.withImportd [" ] [ ''
    ++ lib.optionals systemd.withMachined [ "machine.slice" "machines.target" ]
    ++ lib.optionals systemd.withNspawn [ "systemd-nspawn@.service" ]
    ++ lib.optionals (systemd.withVmspawn or false) [ "systemd-vmspawn@.service" ]
    ++ lib.optionals systemd.withImportd [
  '' ] pruned);
in {
  disabledModules = [ "system/boot/systemd/user.nix" ];
  imports = [ (builtins.toFile "systemd-user-minimal.nix" fixed) ];
}
