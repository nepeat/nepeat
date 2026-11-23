# al_eth_trace: the vendor al_eth module (../src, unchanged on disk) rebuilt
# with every HAL register access, PCI config access and HAL delay recorded,
# (./al_trace_rec.c) readable as text from /proc/al_eth_trace in the difftest format
# (../difftest/mock.h), for comparing the C driver on the hardware with the
# Rust HAL (./parse.py, then ../difftest/difftest.rs --hw --board). Different module and
# PCI driver name (al_eth_trace), so it never shadows al_eth; only one of the
# two can drive the NIC: unload al_eth first.
#   config.boot.kernelPackages.callPackage ./trace { }
{ lib, stdenv, kernel }:
stdenv.mkDerivation {
  pname = "al_eth_trace";
  inherit (kernel) version;
  src = builtins.path { path = ../src; name = "al_eth-src"; };
  tracesrc = builtins.path { path = ./.; name = "al_eth-trace-src"; };
  __structuredAttrs = true;
  strictDeps = true;
  hardeningDisable = [ "all" ];
  nativeBuildInputs = kernel.moduleBuildDependencies;
  makeFlags = kernel.commonMakeFlags ++ [
    "-C" "${kernel.dev}/lib/modules/${kernel.modDirVersion}/build"
    "KCFLAGS=-DCONFIG_AL_ETH_ALPINE_V2"
  ];
  # every edit is checked: a source change that breaks one fails the build
  postPatch = ''
    set -euo pipefail
    cp $tracesrc/al_trace_rec.c $tracesrc/al_eth_trace.h .
    chmod u+w *
    once() { [ "$(grep -c -- "$1" "$2")" = "$3" ] || { echo "al_eth_trace: expected $3 x '$1' in $2" >&2; exit 1; }; }

    # the accessors: al_eth_trace.h last in the platform header
    awk '{ l[NR] = $0 } END { last = 0; for (i = 1; i <= NR; i++) if (l[i] ~ /^#endif/) last = i;
         for (i = 1; i <= NR; i++) { if (i == last) print "#include \"al_eth_trace.h\""; print l[i] } }' \
      al_hal_plat_services.h > t && mv t al_hal_plat_services.h
    once '#include "al_eth_trace.h"' al_hal_plat_services.h 1

    # driver name, BAR bases, entry marks, PCI config accesses, module init/exit
    sed -i 's/^#define DRV_MODULE_NAME.*/#define DRV_MODULE_NAME "al_eth_trace"/' al_eth_main.c
    once '#define DRV_MODULE_NAME "al_eth_trace"' al_eth_main.c 1
    sed -i 's/^\tiomap = pcim_iomap_table(pdev);$/&\n\tal_trace_bars(pdev);/' al_eth_main.c
    once 'al_trace_bars(pdev);' al_eth_main.c 1
    mark() { sed -i "/$1/,/^{$/ s/^{$/{ al_trace_mark($2);/" al_eth_main.c; }
    mark '^al_eth_probe(struct pci_dev \*pdev' AL_TRACE_PROBE
    mark '^static int al_eth_open(struct net_device \*netdev)' AL_TRACE_OPEN
    mark '^static int al_eth_close(struct net_device \*netdev)' AL_TRACE_CLOSE
    mark '^al_eth_remove(struct pci_dev \*pdev)' AL_TRACE_REMOVE
    mark '^static void al_eth_set_rx_mode(struct net_device \*netdev)' AL_TRACE_RX_MODE
    mark '^static int al_eth_change_mtu(struct net_device \*dev, int new_mtu)' AL_TRACE_MTU
    mark '^static int al_eth_set_mac_addr(struct net_device \*dev, void \*p)' AL_TRACE_SET_MAC
    mark '^static void al_eth_get_stats64(struct net_device \*netdev,' AL_TRACE_STATS
    for m in PROBE OPEN CLOSE REMOVE MTU SET_MAC STATS; do once "al_trace_mark(AL_TRACE_$m)" al_eth_main.c 1; done
    once 'al_trace_mark(AL_TRACE_RX_MODE)' al_eth_main.c 2   # the #else stub too
    sed -i 's/^\tpci_read_config_dword((struct pci_dev \*)handle, where, val);$/&\n\tal_trace_cfg(0, where, *val);/' al_eth_main.c
    sed -i 's/^\tpci_write_config_dword((struct pci_dev \*)handle, where, val);$/\tal_trace_cfg(1, where, val);\n&/' al_eth_main.c
    once 'al_trace_cfg(0, where, \*val);' al_eth_main.c 1
    once 'al_trace_cfg(1, where, val);' al_eth_main.c 1
    # hw_stop's direct udelay(100): through the HAL delay so it is recorded
    sed -i 's/^\tudelay(100);$/\tal_udelay(100);/' al_eth_main.c
    once 'al_udelay(100);' al_eth_main.c 1
    sed -i 's/^static int __init al_eth_init(void)$/&\n{\n\tint rc = al_trace_init();\n\n\tif (rc)\n\t\treturn rc;\n\trc = al_eth_init_orig();\n\tif (rc)\n\t\tal_trace_exit();\n\treturn rc;\n}\nstatic int __init al_eth_init_orig(void)/' al_eth_main.c
    sed -i 's/^static int __init al_eth_init(void)$/static int __init al_eth_init_orig(void);\n&/' al_eth_main.c
    sed -i 's/^\tpci_unregister_driver(&al_eth_pci_driver);$/&\n\tal_trace_exit();/' al_eth_main.c
    once 'al_trace_init()' al_eth_main.c 1
    once 'al_trace_exit();' al_eth_main.c 2
    grep -q '^#include "al_eth_trace.h"' al_eth_main.c || sed -i '0,/^#include "al_eth.h"/s//#include "al_eth.h"\n#include "al_eth_trace.h"/' al_eth_main.c

    # the module: al_eth_trace.ko
    sed -i 's/^obj-m := al_eth.o$/obj-m := al_eth_trace.o/; s/^al_eth-y := /al_eth_trace-y := al_trace_rec.o /' Makefile
    once 'obj-m := al_eth_trace.o' Makefile 1
    once 'al_eth_trace-y := al_trace_rec.o' Makefile 1
  '';
  buildPhase = ''
    runHook preBuild
    make "''${makeFlags[@]}" M=$(pwd) modules
    runHook postBuild
  '';
  installPhase = ''
    runHook preInstall
    make "''${makeFlags[@]}" M=$(pwd) INSTALL_MOD_PATH=$out modules_install
    install -Dm644 $tracesrc/parse.py $out/share/al_eth_trace/parse.py
    runHook postInstall
  '';
  meta = {
    description = "al_eth with a register access recorder (test build, never a default)";
    license = lib.licenses.gpl2Only;
    platforms = [ "aarch64-linux" ];
  };
}
