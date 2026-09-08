{
  description = "NixOS kernel + initramfs for Accton AS7772-32X (PowerPC T2080 / BCM56960)";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-25.11";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
    let
      # ------------------------------------------------------------------ #
      # Build runs on the host system,
      # cross-compiling for powerpc64-linux (big-endian, 64-bit)
      # ------------------------------------------------------------------ #
      buildSystem = system;
      targetArch  = "powerpc";   # kernel ARCH= is always "powerpc" for ppc32+ppc64
      lib         = nixpkgs.lib;

      # Host (build-side) packages — native tools (mkimage, dtc, cpio …)
      pkgsBuild = import nixpkgs { system = buildSystem; };

      # Cross-compilation target: ppc64 big-endian (powerpc64-unknown-linux-gnuabielfv1).
      # We import nixpkgs directly with a full crossSystem rather than using
      # pkgsBuild.pkgsCross.ppc64, because the pre-defined ppc64 example in
      # nixpkgs 23.11 does not populate linux-kernel.target, which
      # manual-config.nix requires.  Providing it here also lets nixpkgs know
      # the correct U-Boot image type for this board.
      xpkgs = import nixpkgs {
        system = buildSystem;
        crossSystem = {
          config = "powerpc64-unknown-linux-gnuabielfv1";
          linux-kernel = {
            name        = "powerpc64";
            target      = "zImage.epapr";   # ePAPR boot wrapper for Book3E 64-bit
            DTB         = true;
            autoModules = false;
            baseConfig  = "corenet64_smp_defconfig";
          };
        };
      };

      # ------------------------------------------------------------------ #
      # Kernel patches — same format as boot.kernelPatches in NixOS.
      # Each entry may have:
      #   patch              — path/derivation to apply (null = config-only)
      #   structuredExtraConfig — attrset of Kconfig options without CONFIG_
      #                           prefix; values from lib.kernel (yes/no/module/
      #                           freeform/unset)
      # ------------------------------------------------------------------ #
      as7772KernelPatches = [
        {
          # Board-specific Kconfig options.
          # Applied on top of corenet64_smp_defconfig via buildLinux's configurePhase.
          name  = "accton-as7772-32x-config";
          patch = null;
          ignoreConfigErrors = true;
          structuredExtraConfig = with lib.kernel; {
            # # ── Architecture ─────────────────────────────────────────────
            # # 64-bit PPC (e6500 Book3E core in 64-bit mode)
            # E6500      = no;              # CONFIG_E6500 (not E6500_CPU)

            # ── Console ──────────────────────────────────────────────────
            # earlycon: allows the kernel to open the console immediately on
            # boot using stdout-path from the DTS, before the driver subsystem
            # initialises.  Without this, a crash before serial driver load is
            # completely silent.  Enabled by passing 'earlycon' on the cmdline.
            SERIAL_EARLYCON      = yes;
            SERIAL_8250          = yes;
            SERIAL_8250_CONSOLE  = yes;
            SERIAL_8250_NR_UARTS = freeform "4";
            SERIAL_8250_FSL      = yes;
            SERIAL_OF_PLATFORM   = yes;

            # ── Platform / SoC ───────────────────────────────────────────
            FSL_SOC        = yes;
            FSL_PCI        = yes;
            FSL_IFC        = yes;
            FSL_CORENET_CF = yes;
            FSL_GTM        = yes;
            FSL_RCPM       = yes;

            # ── NOR Flash (CFI via IFC) ──────────────────────────────────
            MTD               = yes;
            MTD_CFI           = yes;
            MTD_CFI_INTELEXT  = yes;
            MTD_CFI_AMDSTD    = yes;
            MTD_PHYSMAP       = yes;
            MTD_PHYSMAP_OF    = yes;
            MTD_CMDLINE_PARTS = yes;
            MTD_OF_PARTS      = yes;

            # ── I2C ──────────────────────────────────────────────────────
            I2C_MUX         = yes;
            I2C_MUX_PCA954x = yes;

            # ── I2C Devices ──────────────────────────────────────────────
            SENSORS_ADT7461 = yes;
            EEPROM_AT24     = yes;
            RTC_CLASS       = yes;
            RTC_DRV_DS1672  = yes;

            # ── GPIO ─────────────────────────────────────────────────────
            GPIOLIB      = yes;
            GPIO_SYSFS   = yes;
            GPIO_MPC8XXX = yes;

            # ── FMan Ethernet (management port FM1@DTSEC1) ───────────────
            FSL_FMAN               = yes;
            FSL_DPAA               = yes;
            FSL_DPAA_ETH           = yes;
            FSL_FMAN_MEM_PARTITION = yes;
            NET                    = yes;
            ETHERNET               = yes;
            NET_VENDOR_FREESCALE   = yes;
            PHYLIB                 = yes;
            BROADCOM_PHY           = yes;   # BCM5400 management PHY

            # ── PCIe ─────────────────────────────────────────────────────
            PCI          = yes;
            PCIEPORTBUS  = yes;
            PCI_MSI      = yes;
            FSL_PCI_INIT = yes;

            # ── USB ──────────────────────────────────────────────────────
            USB               = yes;
            USB_EHCI_HCD      = yes;
            USB_EHCI_FSL      = yes;
            USB_FSL_MPH_DR_OF = yes;
            USB_STORAGE       = yes;

            # ── SD Card (eSDHC) ──────────────────────────────────────────
            MMC                = yes;
            MMC_SDHCI          = yes;
            MMC_SDHCI_OF_ESDHC = yes;

            # ── DMA ──────────────────────────────────────────────────────
            DMADEVICES = yes;
            FSL_DMA    = yes;

            OF                = yes;
            OF_DYNAMIC        = yes;
            OF_EARLY_FLATTREE = yes;
            OF_ADDRESS        = yes;
            OF_IRQ            = yes;
            OF_NET            = yes;

            # ── Debug ────────────────────────────────────────────────────
            PRINTK       = yes;
            EARLY_PRINTK = yes;
            MAGIC_SYSRQ  = yes;
          };
        }
      ];

      # ------------------------------------------------------------------ #
      # Kernel build derivation
      # Starts from xpkgs.linux (nixpkgs default LTS for ppc64, currently 6.1).
      # - argsOverride sets corenet64_smp_defconfig as the base config.
      # - kernelPatches injects the board DTS and board-specific Kconfig options.
      # - overrideAttrs adds a postPatch Makefile entry and custom build/install
      #   phases for the PowerPC-specific uImage + dtbs targets.
      # ------------------------------------------------------------------ #
      kernel = (xpkgs.linux.override {
        kernelPatches = as7772KernelPatches;
        # argsOverride wins over the hardcoded defconfig in linux-6.x.nix
        argsOverride  = { defconfig = "corenet64_smp_defconfig"; };
      }).overrideAttrs (old: {

        # Copy board DTS into the kernel tree and register its DTB.
        postPatch = (old.postPatch or "") + ''
          cp ${./dts/accton_as7772_32x.dts} arch/powerpc/boot/dts/accton_as7772_32x.dts
          echo 'dtb-$(CONFIG_PPC_E500MC) += accton_as7772_32x.dtb' \
              >> arch/powerpc/boot/dts/Makefile
        '';
      });

      # ------------------------------------------------------------------ #
      # Minimal initramfs for NixOS netboot
      # Contains just enough to mount NFS and pivot_root
      # ------------------------------------------------------------------ #
      initramfs = xpkgs.callPackage (
        { stdenv, busybox, kmod, nfs-utils, ... }:
        stdenv.mkDerivation {
          pname   = "as7772-initramfs";
          version = "1.0";

          dontUnpack = true;

          nativeBuildInputs = [ pkgsBuild.cpio pkgsBuild.gzip ];

          buildPhase = ''
            # Build a minimal initramfs tree
            mkdir -p initramfs/{bin,sbin,etc,proc,sys,dev,tmp,newroot,lib,lib/modules}

            # Busybox for shell + basic tools
            cp ${busybox}/bin/busybox initramfs/bin/busybox
            chmod +x initramfs/bin/busybox

            # Create symlinks for essential busybox applets
            for cmd in sh ash mount umount switch_root mkdir mknod \
                       ifconfig ip dhcpcd udhcpc ls cat echo sleep \
                       insmod modprobe dmesg; do
              ln -sf busybox initramfs/bin/$cmd 2>/dev/null || true
            done

            # Init script
            cp ${./initramfs/init} initramfs/init
            chmod +x initramfs/init

            # udhcpc script
            mkdir -p initramfs/etc
            cp ${./initramfs/udhcpc.script} initramfs/etc/udhcpc.script
            chmod +x initramfs/etc/udhcpc.script

            # Pack as cpio
            mkdir -p $out
            cd initramfs
            find . | cpio -H newc -o | gzip -9 > $out/initramfs.cpio.gz
            cd ..

            echo "[as7772] initramfs size: $(ls -lh $out/initramfs.cpio.gz | awk '{print $5}')"
          '';

          installPhase = "true";  # installPhase is done inline above
        }
      ) {};

      # ------------------------------------------------------------------ #
      # FIT image: bundles kernel + initramfs + DTB for U-Boot
      # Matches the format U-Boot already expects (same as original ONIE)
      # ------------------------------------------------------------------ #
      fitImage = pkgsBuild.stdenv.mkDerivation {
        pname   = "as7772-fit-image";
        version = "1.0";

        dontUnpack = true;

        nativeBuildInputs = [ pkgsBuild.ubootTools pkgsBuild.dtc ];

        buildPhase = ''
          cat > as7772.its << ITS
          /dts-v1/;
          / {
            description = "AS7772-32X Linux ${kernel.version} FIT image";
            #address-cells = <1>;

            images {
              kernel {
                description  = "Linux ${kernel.version} for AS7772-32X";
                data         = /incbin/("${kernel}/zImage.epapr");
                type         = "kernel";
                arch         = "powerpc";
                os           = "linux";
                compression  = "none";
                load         = <0x04000000>;
                entry        = <0x04000000>;
                hash { algo = "crc32"; };
              };

              fdt {
                description  = "AS7772-32X device tree (kernel ${kernel.version})";
                data         = /incbin/("${kernel}/dtbs/accton_as7772_32x.dtb");
                type         = "flat_dt";
                arch         = "powerpc";
                compression  = "none";
                hash { algo = "crc32"; };
              };

              ramdisk {
                description  = "NixOS netboot initramfs";
                data         = /incbin/("${initramfs}/initramfs.cpio.gz");
                type         = "ramdisk";
                arch         = "powerpc";
                os           = "linux";
                compression  = "gzip";
                hash { algo = "crc32"; };
              };
            };

            configurations {
              default = "nixos-netboot";

              nixos-netboot {
                description = "AS7772-32X NixOS netboot";
                kernel      = "kernel";
                fdt         = "fdt";
                ramdisk     = "ramdisk";
              };
            };
          };
          ITS

          mkdir -p $out
          mkimage -f as7772.its $out/as7772-nixos-netboot.itb
          cp as7772.its $out/as7772.its

          echo "[as7772] FIT image:"
          ls -lh $out/
          mkimage -l $out/as7772-nixos-netboot.itb
        '';

        installPhase = "true";
      };

    in
    {
      # ── Exposed packages ────────────────────────────────────────────────
      packages = {
        inherit kernel initramfs fitImage;

        # Convenience: everything needed to netboot
        default = fitImage;
      };

      # ── Dev shell for manual kernel config / debugging ─────────────────
      devShells.default = pkgsBuild.mkShell {
        name = "as7772-kernel-dev";

        packages = with pkgsBuild; [
          # Cross toolchain
          pkgsCross.powerpc64.stdenv.cc
          # Kernel build tools
          bison flex perl bc openssl elfutils python3
          # Image tools
          ubootTools dtc binutils
          # Debugging
          file hexdump
        ];

        shellHook = ''
          export ARCH=powerpc
          export CROSS_COMPILE=powerpc64-unknown-linux-gnuabielfv1-
          export KCONFIG_CONFIG=.config

          echo "╔══════════════════════════════════════════════════════╗"
          echo "║  AS7772-32X Kernel Development Shell                ║"
          echo "║  Target: PowerPC e6500 (T2080) + BCM56960           ║"
          echo "╠══════════════════════════════════════════════════════╣"
          echo "║  ARCH=$ARCH                                          ║"
          echo "║  CROSS_COMPILE=$CROSS_COMPILE                        ║"
          echo "╠══════════════════════════════════════════════════════╣"
          echo "║  Quick commands:                                     ║"
          echo "║    make corenet64_smp_defconfig   # start config     ║"
          echo "║    make menuconfig           # interactive config    ║"
          echo "║    make -j$(nproc) uImage dtbs    # build kernel     ║"
          echo "║    make -j$(nproc) modules        # build modules    ║"
          echo "╚══════════════════════════════════════════════════════╝"

          # Convenient alias for TFTP server setup
          alias serve-tftp='cp result/as7772-nixos-netboot.itb /var/lib/tftpboot/ && echo "Copied to TFTP root"'
        '';
      };
    }) // {
      # ── NixOS module (global, not per-system) ───────────────────────────
      nixosModules.as7772-netboot = { config, lib, pkgs, ... }: {
        options.as7772.netboot = {
          enable       = lib.mkEnableOption "AS7772-32X netboot server";
          tftpRoot     = lib.mkOption { type = lib.types.str; default = "/var/lib/tftpboot"; };
          nfsExport    = lib.mkOption { type = lib.types.str; default = "/exports/as7772"; };
          switchSubnet = lib.mkOption { type = lib.types.str; default = "192.168.1.0/24"; };
        };

        config = lib.mkIf config.as7772.netboot.enable {
          # TFTP server
          services.atftpd = {
            enable = true;
            root   = config.as7772.netboot.tftpRoot;
          };

          # NFS server
          services.nfs.server = {
            enable  = true;
            exports = ''
              ${config.as7772.netboot.nfsExport} \
                ${config.as7772.netboot.switchSubnet}(rw,no_root_squash,no_subtree_check,vers=3)
            '';
          };

          # DHCP - serves the switch with its known MAC
          services.dhcpd4 = {
            enable     = true;
            interfaces = [ "eth0" ];
            extraConfig = ''
              option domain-name-servers 1.1.1.1;
              default-lease-time 3600;

              subnet 192.168.1.0 netmask 255.255.255.0 {
                range 192.168.1.100 192.168.1.200;
                option routers 192.168.1.1;

                # AS7772-32X - identified by known MAC from syseeprom
                host as7772-switch {
                  hardware ethernet 1c:ea:0b:38:2b:d5;
                  fixed-address     192.168.1.10;
                  # U-Boot will pick this up as the boot file
                  filename          "as7772-nixos-netboot.itb";
                  next-server       192.168.1.99;
                }
              }
            '';
          };

          # Open required firewall ports
          networking.firewall.allowedUDPPorts = [ 67 68 69 ];  # DHCP + TFTP
          networking.firewall.allowedTCPPorts = [ 2049 111 ];  # NFS

          # Symlink the FIT image into TFTP root
          system.activationScripts.as7772-tftp = ''
            mkdir -p ${config.as7772.netboot.tftpRoot}
            ln -sf ${self.packages.${pkgs.system}.fitImage}/as7772-nixos-netboot.itb \
              ${config.as7772.netboot.tftpRoot}/as7772-nixos-netboot.itb
          '';
        };
      };
    };
}
