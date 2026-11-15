# Flex counter polling for services.sonic: syncd polls only the groups
# enabled in CONFIG_DB FLEX_COUNTER_TABLE (SONiC seeds them from
# init_cfg.json; without them "show interfaces counters" stays empty until
# someone runs counterpoll). Seeded (config-ownership.md): the Nix value goes
# in only where the user has none, so "counterpoll port disable" + "config
# save" sticks.
{ config, lib, ... }:
let
  inherit (lib) mkOption mkIf mkDefault types mapAttrs;
  cfg = config.services.sonic;
in {
  options.services.sonic.flexCounters = mkOption {
    type = types.attrsOf (types.attrsOf types.str);
    # SONiC's init_cfg enables ~12 groups; sai-openbcm answers port stats
    # only (no queue/PG/RIF/ACL/trap counters: those groups would only make
    # syncd log failed counter creates).
    default = { PORT.FLEX_COUNTER_STATUS = "enable"; };
    example = { PORT = { FLEX_COUNTER_STATUS = "enable"; POLL_INTERVAL = "1000"; }; RIF.FLEX_COUNTER_STATUS = "enable"; };
    description = "FLEX_COUNTER_TABLE groups (key -> fields) seeded into CONFIG_DB.";
  };

  config = mkIf cfg.enable {
    services.sonic.settings.FLEX_COUNTER_TABLE = mapAttrs (_: mapAttrs (_: mkDefault)) cfg.flexCounters;
    services.sonic.tableModes.FLEX_COUNTER_TABLE = mkDefault "seed";
  };
}
