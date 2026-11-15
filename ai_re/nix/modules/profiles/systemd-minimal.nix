{ modulesPath, ... }:
let
  # Pinned unstable's user module unconditionally requests four units omitted
  # by systemd when machined/nspawn/vmspawn are disabled. Mirror the guards
  # already used for the corresponding system units; keep every other option.
  original = builtins.readFile (modulesPath + "/system/boot/systemd/user.nix");
  pruned = builtins.replaceStrings [
    "    \"machine.slice\"\n"
    "    \"machines.target\"\n"
    "    \"systemd-nspawn@.service\"\n"
    "    \"systemd-vmspawn@.service\"\n"
  ] [ "" "" "" "" ] original;
  fixed = builtins.replaceStrings [ "  ++ lib.optionals systemd.withImportd [" ] [ ''
    ++ lib.optionals systemd.withMachined [ "machine.slice" "machines.target" ]
    ++ lib.optionals systemd.withNspawn [ "systemd-nspawn@.service" ]
    ++ lib.optionals (systemd.withVmspawn or false) [ "systemd-vmspawn@.service" ]
    ++ lib.optionals systemd.withImportd [
  '' ] pruned;
in {
  disabledModules = [ "system/boot/systemd/user.nix" ];
  imports = [ (builtins.toFile "systemd-user-minimal.nix" fixed) ];
}
