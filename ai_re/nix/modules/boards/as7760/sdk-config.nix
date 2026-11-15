# AS7760-32X Broadcom SDK config (config.bcm) for services.sonic.sai.sdkConfig:
# the public sonic-buildimage Accton AS7712-32X Tomahawk config (Apache-2.0,
# pinned in packages/sonic/sources.json), re-split per cage from the SONiC
# ports by packages/openbcm/configs (Tomahawk rules and SDK sources there).
# Checked against the factory configs (devices/sonic-nixos/PROGRESS.md,
# 2026-11-15): the 32x100G and 128x25G layouts give the factory's portmap,
# lane map and polarity keys; the public pre-emphasis equals the factory
# script's per-lane taps (pre/main/post) on all 128 lanes.
#   import ./sdk-config.nix { inherit openbcm sonic; } { ports = [ ... ]; settings = { }; }
# Keys beyond the public file, decided per key (factory 32x100G config):
#   management ports  66=129:10 and 100=131:10 kept (xe0/xe1 as the bring-up ran
#                     them; sai-openbcm hides them). The factory's 67=133:10 and
#                     101=134:10 are dropped: 67/101 are fixed loopback ports in
#                     SDK 6.5.27, so the SDK ignores those lines.
#   dport_map_*       generated (diag-shell numbers in front-panel order, as
#                     the factory has); only the diag shell uses them.
#   pbmp_*            generated from the ports (pbmp_xport_xe must hold every
#                     port or it comes up HiGig).
#   serdes_driver_current (public: 0x8 on every lane) left out: the factory
#                     never sets the TX amplitude, so every run so far used the
#                     SDK default; pre-emphasis (pre/main/post) is kept. The
#                     factory script's post2 taps have no config.bcm key.
#   ccm_dma_enable=0, ccmdma_intr_enable=0, mem_cache_enable=0
#                     kept: what every hardware run so far used (no SDK table
#                     cache in host memory).
# The public file's own globals (L3 ALPM, bcm_num_cos=10, timeouts, ...)
# stay; none of them ran on this switch before. settings (from
# services.sonic.sai.sdkConfig.settings) wins; null removes a key.
{ openbcm, sonic }:
{ ports, settings ? { } }:
openbcm.configs.tomahawk {
  name = "as7760-32x-config.bcm";
  base = "${sonic.sources.buildimage}/device/accton/x86_64-accton_as7712_32x-r0/Accton-AS7712-32X/th-as7712-32x100G.config.bcm";
  label = "sonic-buildimage ${builtins.substring 0 12 sonic.sources.buildimage.rev} device/accton/x86_64-accton_as7712_32x-r0/Accton-AS7712-32X/th-as7712-32x100G.config.bcm";
  inherit ports;
  laneKeys = [ "preemphasis" ];
  management = [
    { logical = 66; phys = 129; speed = 10; }
    { logical = 100; phys = 131; speed = 10; }
  ];
  settings = {
    ccm_dma_enable = "0";
    ccmdma_intr_enable = "0";
    mem_cache_enable = "0";
  } // settings;
}
