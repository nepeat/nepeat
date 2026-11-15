# Mainline kernel for Annapurna Labs Alpine boards, two variants, plus board
# DTBs compiled apart from the kernel (a DTS edit doesn't rebuild it).
#   linuxAlpine = import ./linux-alpine { inherit pkgs lib; variant = "trimmed"; };
#   linuxAlpine.kernelPackages, linuxAlpine.alEth, linuxAlpine.dtbs { name; dts; }
# - "trimmed" (default, first bring-up): nixpkgs' config minus every foreign
#   ARCH_* (it enables ~50 SoC families: Qualcomm, MediaTek, Tesla FSD, ...) and
#   subsystems these boxes can't use. Generic PCI/USB/net/fs drivers stay, so a
#   driver nobody thought of is still there.
# - "minimal": allnoconfig + explicit (the pattern of linux-qoriq-t2080). The
#   list is what the factory kernel bound on DMC-12 u01 plus NixOS's needs;
#   switch to it once a boot has confirmed what's used.
# Board-only drivers go in `boardConfig` (both variants).
{ pkgs, lib, boardConfig ? { }, variant ? "trimmed" }:
let
  minimalKernel = pkgs.linux_latest.override {
    defconfig = "allnoconfig";
    enableCommonConfig = false;
    autoModules = false;
    preferBuiltin = true;
    ignoreConfigErrors = false;
    buildDTBs = false;
    features = { efiBootStub = false; ia32Emulation = false; };
    structuredExtraConfig = with lib.kernel; {
      # --- Alpine V2/V3 SoC: 4x Cortex-A57 (v2) / A72 (v3) ----------------------
      ARCH_ALPINE = yes;
      ALPINE_MSI = yes; # MSI-X domain for al_eth / external PCIe
      SMP = yes;
      NR_CPUS = freeform "16";
      ARM64_4K_PAGES = yes;
      COMMON_CLK = yes; # fixed-clock sbclk
      ARM_AMBA = yes; # PL061, sp805
      PCI = yes;
      PCI_MSI = yes;
      PCI_IOV = yes; # al_eth calls pci_enable_sriov
      PCIEPORTBUS = yes;
      PCIEAER = yes;
      PCI_HOST_GENERIC = yes; # internal bus (ecam-generic)
      PCIE_AL = yes; # external ports
      PCIE_DW_HOST = yes;
      SERIAL_8250 = yes;
      SERIAL_8250_CONSOLE = yes;
      SERIAL_OF_PLATFORM = yes;
      SERIAL_EARLYCON = yes;
      SERIAL_8250_NR_UARTS = freeform "4";
      SERIAL_8250_RUNTIME_UARTS = freeform "4";
      GPIOLIB = yes;
      GPIO_CDEV = yes;
      GPIO_PL061 = yes;
      WATCHDOG = yes;
      ARM_SP805_WATCHDOG = yes; # the platform restart path (PSCI v0.1 has no SYSTEM_RESET)
      I2C = yes;
      I2C_CHARDEV = yes;
      I2C_DESIGNWARE_CORE = yes;
      I2C_DESIGNWARE_PLATFORM = yes;
      I2C_MUX = yes;
      I2C_MUX_PCA954x = yes;
      SPI = yes;
      SPI_DESIGNWARE = yes;
      SPI_DW_MMIO = yes;
      MTD = yes;
      MTD_SPI_NOR = yes;
      ATA = yes;
      SATA_AHCI = yes;
      SCSI = yes;
      BLK_DEV_SD = yes;
      CHR_DEV_SG = yes;
      THERMAL = yes; # al_eth includes linux/thermal.h
      HWMON = yes;
      # al_eth (out of tree, module): phylib + MDIO.
      NETDEVICES = yes;
      ETHERNET = yes;
      PHYLIB = yes;
      MDIO_DEVICE = yes;
      FIXED_PHY = yes;
      MODULES = yes;
      MODULE_UNLOAD = yes;

      # --- NixOS / systemd contract (from linux-qoriq-t2080) --------------------
      EXPERT = yes;
      PRINTK = yes;
      BUG = yes;
      MULTIUSER = yes;
      FUTEX = yes;
      POSIX_TIMERS = yes;
      EPOLL = yes;
      SIGNALFD = yes;
      TIMERFD = yes;
      INOTIFY_USER = yes;
      FILE_LOCKING = yes;
      SHMEM = yes;
      AIO = yes;
      IO_URING = yes;
      ADVISE_SYSCALLS = yes;
      KALLSYMS = yes;
      BINFMT_ELF = yes;
      BINFMT_SCRIPT = yes;
      COREDUMP = yes;
      BLK_DEV_INITRD = yes;
      RD_GZIP = yes;
      RD_ZSTD = yes; # boot.initrd.compressor = "zstd"
      DEVTMPFS = yes;
      DEVTMPFS_MOUNT = yes;
      TMPFS = yes;
      TMPFS_POSIX_ACL = yes;
      TMPFS_XATTR = yes;
      PROC_FS = yes;
      SYSFS = yes;
      SYSCTL = yes;
      UNIX98_PTYS = yes;
      TTY = yes;
      UNIX = yes;
      NET = yes;
      PACKET = yes;
      INET = yes;
      IPV6 = yes;
      NETFILTER = yes;
      NETFILTER_ADVANCED = yes;
      NF_TABLES = yes;
      NF_CONNTRACK = yes;
      NFT_CT = yes;
      NFT_LOG = yes;
      NF_LOG_SYSLOG = yes;
      NFT_REJECT = yes;
      NFT_REJECT_INET = yes;
      NF_TABLES_INET = yes;
      NFT_FIB_IPV4 = yes;
      NFT_FIB_IPV6 = yes;
      NFT_FIB_INET = yes;
      BLOCK = yes;
      BLK_DEV = yes;
      BLK_DEV_LOOP = yes;
      # Online resize2fs (first-boot root growth) opens the mounted root rw.
      BLK_DEV_WRITE_MOUNTED = yes;
      MSDOS_PARTITION = yes;
      EFI_PARTITION = yes;
      AUTOFS_FS = yes;
      EXT4_FS = yes;
      EXT4_FS_POSIX_ACL = yes;
      VFAT_FS = yes;
      NLS = yes;
      NLS_CODEPAGE_437 = yes;
      NLS_ISO8859_1 = yes;
      MISC_FILESYSTEMS = yes;
      # RAM-root NixOS (profile-netboot-root): zstd squashfs under a tmpfs overlay.
      SQUASHFS = yes;
      SQUASHFS_ZSTD = yes;
      SQUASHFS_COMPILE_DECOMP_MULTI_PERCPU = yes;
      OVERLAY_FS = yes;
      FW_LOADER = yes;
      IKCONFIG = yes;
      IKCONFIG_PROC = yes;
      DEBUG_FS = yes;
      DEVMEM = yes;
      CGROUPS = yes;
      CGROUP_PIDS = yes;
      CGROUP_FREEZER = yes;
      CGROUP_DEVICE = yes;
      CGROUP_CPUACCT = yes;
      CGROUP_SCHED = yes;
      FAIR_GROUP_SCHED = yes;
      CFS_BANDWIDTH = yes;
      MEMCG = yes;
      PSI = yes; # systemd-oomd
      NAMESPACES = yes;
      UTS_NS = yes;
      IPC_NS = yes;
      PID_NS = yes;
      NET_NS = yes;
      USER_NS = yes;
      SECCOMP = yes;
      SECCOMP_FILTER = yes;
      SYSVIPC = yes;
      FHANDLE = yes;
      BPF = yes;
      BPF_SYSCALL = yes;
      CGROUP_BPF = yes;
      SECURITY = yes;
      SECURITYFS = yes;
      KEYS = yes;
      CRYPTO = yes;
      CRYPTO_USER_API_HASH = yes;
      CRYPTO_USER_API_SKCIPHER = yes;
      CRYPTO_HMAC = yes;
      CRYPTO_AES = yes;
      CRYPTO_SHA256 = yes;
      # Keep symbols for diagnosis; omit bulky DWARF/BTF, LTO and Rust.
      DEBUG_INFO_NONE = yes;
      CC_OPTIMIZE_FOR_SIZE = yes;
    } // boardConfig;
  };
  # Foreign SoC families nixpkgs turns on for aarch64 (7.3.4 config, 2026-11-15);
  # their sub-options (R8A*, TEGRA_*_SOC, ...) depend on these.
  foreignArchs = [
    "ACTIONS" "AIROHA" "SUNXI" "APPLE" "ARTPEC" "ASPEED" "AXIADO" "BCM" "BCM2835"
    "BCM_IPROC" "BCMBCA" "BRCMSTB" "BERLIN" "BLAIZE" "BST" "CIX" "EXYNOS" "K3"
    "LG1K" "HISI" "KEEMBAY" "MEDIATEK" "MESON" "MICROCHIP" "SPARX5" "MVEBU" "NXP"
    "LAYERSCAPE" "MXC" "S32" "MA35" "NPCM" "QCOM" "REALTEK" "RENESAS" "ROCKCHIP"
    "SEATTLE" "INTEL_SOCFPGA" "SOPHGO" "STM32" "SYNQUACER" "TEGRA" "TESLA_FSD"
    "SPRD" "THUNDER" "THUNDER2" "UNIPHIER" "VEXPRESS" "VISCONTI" "XGENE" "ZYNQMP"
  ];
  trimmedKernel = pkgs.linuxPackages_latest.kernel.override {
    ignoreConfigErrors = true;
    structuredExtraConfig = with lib.kernel;
      lib.genAttrs (map (a: "ARCH_${a}") foreignArchs) (_: lib.mkForce no) // {
      # Alpine SoC plumbing and boot-critical drivers: built in.
      ARCH_ALPINE = lib.mkForce yes;
      ALPINE_MSI = lib.mkForce yes;
      PCI_HOST_GENERIC = lib.mkForce yes;
      PCIE_AL = lib.mkForce yes;
      PCIE_DW_HOST = lib.mkForce yes;
      SERIAL_8250 = lib.mkForce yes;
      SERIAL_8250_CONSOLE = lib.mkForce yes;
      SERIAL_OF_PLATFORM = lib.mkForce yes;
      ATA = lib.mkForce yes;
      SATA_AHCI = lib.mkForce yes;
      SCSI = lib.mkForce yes;
      BLK_DEV_SD = lib.mkForce yes;
      EXT4_FS = lib.mkForce yes;
      BLK_DEV_LOOP = lib.mkForce yes;
      SQUASHFS = lib.mkForce yes;
      SQUASHFS_ZSTD = lib.mkForce yes;
      OVERLAY_FS = lib.mkForce yes;
      I2C_CHARDEV = lib.mkForce yes;
      I2C_DESIGNWARE_CORE = lib.mkForce yes;
      I2C_DESIGNWARE_PLATFORM = lib.mkForce yes;
      I2C_MUX = lib.mkForce yes;
      I2C_MUX_PCA954x = lib.mkForce yes;
      SPI_DESIGNWARE = lib.mkForce yes;
      SPI_DW_MMIO = lib.mkForce yes;
      MTD = lib.mkForce yes;
      MTD_SPI_NOR = lib.mkForce yes;
      GPIO_PL061 = lib.mkForce yes;
      WATCHDOG = lib.mkForce yes;
      ARM_SP805_WATCHDOG = lib.mkForce yes;
      # Subsystems with nothing behind them on these boxes. I3C off also lets
      # I3C_OR_I2C follow I2C (=y), so I2C sensors can be built in.
      I3C = lib.mkForce no;
      DRM = lib.mkForce no;
      FB = lib.mkForce no;
      SOUND = lib.mkForce no;
      WLAN = lib.mkForce no;
      CFG80211 = lib.mkForce no;
      MAC80211 = lib.mkForce no;
      WWAN = lib.mkForce no;
      IIO = lib.mkForce no;
      BT = lib.mkForce no;
      NFC = lib.mkForce no;
      INFINIBAND = lib.mkForce no;
      CAN = lib.mkForce no;
      MEDIA_SUPPORT = lib.mkForce no;
      RC_CORE = lib.mkForce no;
      HAMRADIO = lib.mkForce no;
      ISDN = lib.mkForce no;
      ATM = lib.mkForce no;
      IEEE802154 = lib.mkForce no;
      GNSS = lib.mkForce no;
      FPGA = lib.mkForce no;
      STAGING = lib.mkForce no;
      XEN = lib.mkForce no;
      HYPERV = lib.mkForce no;
      INPUT_TOUCHSCREEN = lib.mkForce no;
      INPUT_JOYSTICK = lib.mkForce no;
      INPUT_TABLET = lib.mkForce no;
    } // lib.mapAttrs (_: lib.mkForce) boardConfig;
  };
  kernel = if variant == "minimal" then minimalKernel else trimmedKernel;
  kp = pkgs.linuxPackagesFor kernel;

  dtsIncludes = pkgs.runCommand "alpine-dts-includes-${kernel.version}" { } ''
    mkdir -p $out
    tar -xf ${kernel.src} -C $out --strip-components=1 --wildcards \
      '*/arch/arm64/boot/dts/amazon/*' \
      '*/include/dt-bindings/*'
  '';
in
{
  inherit kernel;
  kernelPackages = kp;
  alEth = kp.callPackage ../al-eth { };
  # One board DTB in an `amazon/` dir, the layout hardware.deviceTree expects.
  dtbs = { name, dts }:
    pkgs.runCommand "${name}-dtbs-${kernel.version}" {
      nativeBuildInputs = [ pkgs.buildPackages.dtc pkgs.buildPackages.stdenv.cc ];
    } ''
      mkdir -p $out/amazon
      $CC -E -nostdinc -undef -D__DTS__ -x assembler-with-cpp \
        -I ${dtsIncludes}/arch/arm64/boot/dts/amazon \
        -I ${dtsIncludes}/include \
        ${dts} -o board.pp.dts
      dtc -I dts -O dtb -@ -W no-unique_unit_address -o $out/amazon/${name}.dtb board.pp.dts
    '';
}
