# Minimal built-in kernel for NXP QorIQ T2080 (e6500, Book E, big endian)
# boards: allnoconfig + exactly what the platform needs. Board deltas go in
# boardPatches (./boards/<board>/), not in this file.
# rust: CONFIG_RUST through the shared ppc64 big-endian series
# (../kernel-rust-ppc64be). Off here; the boards' kernel has it on through
# hardware.qoriqT2080.rust (modules/platforms/qoriq-t2080.nix, default true since
# 2026-11-23). extraConfig: more structuredExtraConfig (tests, QEMU platform).
{ pkgs, lib, boardPatches ? [ ], rust ? false, extraConfig ? { } }:
let
  rustPpc64be = import ../kernel-rust-ppc64be { inherit lib; };
in
(if rust then rustPpc64be.keepBindings else lib.id) (pkgs.linux_latest.override {
  defconfig = "allnoconfig";
  enableCommonConfig = false;
  autoModules = false;
  preferBuiltin = true;
  ignoreConfigErrors = false;
  buildDTBs = false;
  kernelPatches = boardPatches ++ lib.optionals rust rustPpc64be.kernelPatches;
  features = { efiBootStub = false; ia32Emulation = false; };
  structuredExtraConfig = with lib.kernel; {
    # T2080/e6500: 4 cores, two threads each, 16 GiB physical RAM.
    PPC64 = yes;
    PPC_BOOK3E_64 = yes;
    E6500_CPU = yes;
    FSL_SOC_BOOKE = yes;
    CORENET_GENERIC = yes;
    CPU_BIG_ENDIAN = yes;
    PPC64_BIG_ENDIAN_ELF_ABI_V2 = yes;
    ALTIVEC = yes;
    SMP = yes;
    NR_CPUS = freeform "8";
    COMPAT = yes;
    PHYS_64BIT = yes;
    SWIOTLB = yes;
    CPU_FREQ = yes;
    QORIQ_CPUFREQ = yes;
    CPU_FREQ_DEFAULT_GOV_PERFORMANCE = yes;
    # Boot and basic systemd/userspace contracts.
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
    DEVTMPFS = yes;
    DEVTMPFS_MOUNT = yes;
    TMPFS = yes;
    TMPFS_POSIX_ACL = yes;
    PROC_FS = yes;
    SYSFS = yes;
    SYSCTL = yes;
    UNIX98_PTYS = yes;
    TTY = yes;
    SERIAL_8250 = yes;
    SERIAL_8250_CONSOLE = yes;
    SERIAL_OF_PLATFORM = yes;
    SERIAL_8250_NR_UARTS = freeform "4";
    SERIAL_8250_RUNTIME_UARTS = freeform "4";
    UNIX = yes;
    NET = yes;
    PACKET = yes;
    INET = yes;
    IPV6 = yes;
    NETDEVICES = yes;
    ETHERNET = yes;
    NET_VENDOR_FREESCALE = yes;
    FSL_DPAA = yes;
    FSL_FMAN = yes;
    FSL_DPAA_ETH = yes;
    FSL_XGMAC_MDIO = yes;
    MARVELL_PHY = yes;
    BROADCOM_PHY = yes;
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
    # Switch control planes (SONiC's vlanmgrd/teammgrd/intfmgrd, also plain
    # Linux bridging): modules, loaded only when used.
    BRIDGE = module;
    BRIDGE_VLAN_FILTERING = yes;
    VLAN_8021Q = module;
    NET_TEAM = module;
    NET_TEAM_MODE_LOADBALANCE = module;
    NET_TEAM_MODE_ACTIVEBACKUP = module;
    NET_TEAM_MODE_ROUNDROBIN = module;
    NET_TEAM_MODE_BROADCAST = module;
    DUMMY = module;
    # VRF devices (SONiC's vrfmgrd). NET_VRF depends on IP_MULTIPLE_TABLES
    # (which depends on IP_ADVANCED_ROUTER), NET_L3_MASTER_DEV and, with
    # IPv6, IPV6_MULTIPLE_TABLES; allnoconfig has none of them.
    IP_ADVANCED_ROUTER = yes;
    IP_MULTIPLE_TABLES = yes;
    IPV6_MULTIPLE_TABLES = yes;
    NET_L3_MASTER_DEV = yes;
    NET_VRF = module;
    # SoC PCIe, storage and diagnostic buses.
    PCI = yes;
    PCI_MSI = yes;
    PCIEPORTBUS = yes;
    PCIEAER = yes;
    FSL_PCI = yes;
    IOMMU_SUPPORT = yes;
    FSL_PAMU = yes;
    MEMORY = yes;
    FSL_IFC = yes;
    MMC = yes;
    MMC_BLOCK = yes;
    MMC_SDHCI = yes;
    MMC_SDHCI_PLTFM = yes;
    MMC_SDHCI_OF_ESDHC = yes;
    USB = yes;
    USB_SUPPORT = yes;
    USB_EHCI_HCD = yes;
    USB_EHCI_FSL = yes;
    USB_STORAGE = yes;
    USB_UAS = yes;
    SCSI = yes;
    CHR_DEV_SG = yes;
    BLK_DEV_SD = yes;
    ATA = yes;
    SATA_FSL = yes;
    BLOCK = yes;
    BLK_DEV = yes;
    BLK_DEV_LOOP = yes;
    # allnoconfig leaves this off; online resize2fs (first-boot root growth)
    # must open the mounted root partition read-write.
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
    I2C = yes;
    I2C_CHARDEV = yes;
    I2C_MPC = yes;
    I2C_MUX = yes;
    I2C_MUX_PCA954x = yes;
    EEPROM_AT24 = yes;
    NVMEM = yes;
    NVMEM_SYSFS = yes;
    HWMON = yes;
    SENSORS_LM90 = yes;
    SENSORS_LM75 = yes;
    # TPM 1.2 on I2C (ST33ZP24; AS7772-32X I2C2 0x13). Board-agnostic: binds
    # only where a device tree has an "st,st33zp24-i2c" node.
    TCG_TPM = yes;
    TCG_TIS_ST33ZP24_I2C = yes;
    RTC_CLASS = yes;
    RTC_DRV_DS1672 = yes;
    GPIOLIB = yes;
    GPIO_CDEV = yes;
    GPIO_MPC8XXX = yes;
    SPI = yes;
    SPI_GPIO = yes;
    SPI_FSL_ESPI = yes;
    SPI_SPIDEV = module;
    MTD = yes;
    MTD_BLOCK = module;
    MTD_CFI = yes;
    MTD_CFI_INTELEXT = yes;
    # Live CFI query reports command set 0002 despite manufacturer ID 0x89.
    MTD_CFI_AMDSTD = yes;
    MTD_PHYSMAP = yes;
    MTD_PHYSMAP_OF = yes;
    JFFS2_FS = yes;
    MISC_FILESYSTEMS = yes;
    # RAM-root NixOS (netboot.nix): zstd squashfs store under a tmpfs overlay.
    SQUASHFS = yes;
    SQUASHFS_ZSTD = yes;
    SQUASHFS_COMPILE_DECOMP_MULTI_PERCPU = yes;
    OVERLAY_FS = yes;
    FW_LOADER = yes;
    MODULES = yes;
    MODULE_UNLOAD = yes;
    IKCONFIG = yes;
    IKCONFIG_PROC = yes;
    DEBUG_FS = yes;
    DEVMEM = yes;
    # cgroup v2 / namespaces / sandboxing required by the service manager.
    CGROUPS = yes;
    CGROUP_PIDS = yes;
    CGROUP_FREEZER = yes;
    CGROUP_DEVICE = yes;
    CGROUP_CPUACCT = yes;
    CGROUP_SCHED = yes;
    FAIR_GROUP_SCHED = yes;
    CFS_BANDWIDTH = yes;
    MEMCG = yes;
    PSI = yes; # systemd-oomd is enabled by the NixOS base profile
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
    CRYPTO_AES = yes;
    CRYPTO_SHA256 = yes;
    # Keep symbols for diagnosis; omit bulky DWARF/BTF, LTO and Rust.
    DEBUG_INFO_NONE = yes;
    CC_OPTIMIZE_FOR_SIZE = yes;
  } // lib.optionalAttrs rust rustPpc64be.config // extraConfig;
})
