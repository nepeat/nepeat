#
# BoardConfig.mk — Amazon "yacht" / KFYAWI (Fire HD 10 11th gen, internal variant)
# MediaTek MT8183.
#
# Every value below is derived from the device or from its own stock
# recovery.img (sha256 6ec64a2d…), not copied from a similar device.
# See README.md for the provenance of each block.
#

DEVICE_PATH := device/amazon/yacht

# --- Platform ----------------------------------------------------------------
# ro.board.platform
TARGET_BOARD_PLATFORM := mt8183
TARGET_NO_BOOTLOADER  := true
TARGET_BOOTLOADER_BOARD_NAME := mt8183

# --- Architecture ------------------------------------------------------------
# The kernel is arm64 (decompressed Image carries the ARM\x64 magic at +56),
# but userspace is 32-bit ONLY: ro.product.cpu.abilist64 is empty, the device
# runs zygote32, and the stock /sbin/recovery is "ELF 32-bit LSB, ARM, EABI5".
# So build a 32-bit recovery userspace on a 64-bit kernel.
TARGET_ARCH                  := arm
# A 32-bit userspace can still target the ARMv8-A ISA. The Pie build system
# derives this from cortex-a53 and otherwise warns that armv7-a-neon is ignored.
TARGET_ARCH_VARIANT          := armv8-a
TARGET_CPU_ABI               := armeabi-v7a
TARGET_CPU_ABI2              := armeabi
# MT8183 = 4x A73 + 4x A53
TARGET_CPU_VARIANT           := cortex-a53
TARGET_CPU_VARIANT_RUNTIME   := cortex-a53
TARGET_USES_64_BIT_BINDER    := true

# --- Kernel ------------------------------------------------------------------
# Geometry read directly out of the stock recovery boot image header.
BOARD_KERNEL_BASE           := 0x40078000
BOARD_KERNEL_PAGESIZE       := 2048
BOARD_KERNEL_OFFSET         := 0x00008000
BOARD_RAMDISK_OFFSET        := 0x14f88000
BOARD_KERNEL_TAGS_OFFSET    := 0x13f88000
BOARD_BOOT_HEADER_VERSION   := 1

# Stock recovery command line, except that the Pie build system appends its own
# buildvariant=eng. veritykeyid must be preserved: LK/dm-verity reference it,
# and dropping it changes boot behaviour.
BOARD_KERNEL_CMDLINE := bootopt=64S3,32N2,64N2
BOARD_KERNEL_CMDLINE += veritykeyid=id:4be33f8ba0062faa6f2d75b5f6475b106e02b7aa

BOARD_MKBOOTIMG_ARGS := --base $(BOARD_KERNEL_BASE)
BOARD_MKBOOTIMG_ARGS += --pagesize $(BOARD_KERNEL_PAGESIZE)
BOARD_MKBOOTIMG_ARGS += --kernel_offset $(BOARD_KERNEL_OFFSET)
BOARD_MKBOOTIMG_ARGS += --ramdisk_offset $(BOARD_RAMDISK_OFFSET)
BOARD_MKBOOTIMG_ARGS += --tags_offset $(BOARD_KERNEL_TAGS_OFFSET)
BOARD_MKBOOTIMG_ARGS += --second_offset 0x00e88000
BOARD_MKBOOTIMG_ARGS += --header_version $(BOARD_BOOT_HEADER_VERSION)
BOARD_MKBOOTIMG_ARGS += --os_version 9.0.0
BOARD_MKBOOTIMG_ARGS += --os_patch_level 2022-01-01

# The stock kernel blob is gzip(Image) followed by FOUR concatenated DTBs
# (0xd00dfeed at +0x000000, +0x02d1a3, +0x05a441, +0x087899) — MTK multi-board.
# We have no kernel source for this build, so reuse the stock blob verbatim.
BOARD_KERNEL_IMAGE_NAME  := Image.gz-dtb
TARGET_PREBUILT_KERNEL   := $(DEVICE_PATH)/prebuilt/Image.gz-dtb

# --- Partitions --------------------------------------------------------------
# Sizes are the exact byte counts of the full-partition dumps.
BOARD_BOOTIMAGE_PARTITION_SIZE     := 33554432
BOARD_RECOVERYIMAGE_PARTITION_SIZE := 42958848
BOARD_FLASH_BLOCK_SIZE             := 131072

# Single-slot, non-dynamic, no vbmeta partition on this device.
AB_OTA_UPDATER := false
BOARD_USES_METADATA_PARTITION := true
TARGET_USERIMAGES_USE_EXT4 := true
TARGET_USERIMAGES_USE_F2FS := true

# --- Verified boot -----------------------------------------------------------
# AVB 1.0 / android-verity via the veritykeyid cmdline; there is no vbmeta
# partition to disable, so do NOT set BOARD_AVB_ENABLE.
BOARD_AVB_ENABLE := false

# --- Recovery ----------------------------------------------------------------
BOARD_HAS_NO_SELECT_BUTTON := true
TARGET_NO_RECOVERY := false
TARGET_RECOVERY_PIXEL_FORMAT := "RGBX_8888"
TARGET_RECOVERY_FSTAB := $(DEVICE_PATH)/recovery/root/system/etc/twrp.fstab

# --- TWRP --------------------------------------------------------------------
# Panel is 1200x1920 portrait-native at density 240 (wm size / wm density).
TW_THEME                       := portrait_hdpi
TW_EXTRA_LANGUAGES             := true
TW_SCREEN_BLANK_ON_BOOT        := true
TW_INPUT_BLACKLIST             := "hbtp_vm"
TW_USE_TOOLBOX                 := true
TW_INCLUDE_NTFS_3G             := true
TW_INCLUDE_FUSE_EXFAT          := true
TW_NO_SCREEN_BLANK             := true
TW_DEFAULT_BRIGHTNESS          := 100
TW_MAX_BRIGHTNESS              := 255
TW_BRIGHTNESS_PATH             := "/sys/class/leds/lcd-backlight/brightness"
TW_INCLUDE_REPACKTOOLS         := true
TW_INCLUDE_RESETPROP           := true

# Encryption: the unit dumped was ro.crypto.state=unencrypted, but the stock
# fstab declares metadata encryption with aes-256-cts filenames. Left OFF until
# it can be tested on an encrypted unit — enabling it blind risks failing to
# mount /data.
TW_INCLUDE_CRYPTO := false

# No A/B, no system-as-root remount tricks needed.
BOARD_SUPPRESS_SECURE_ERASE := true
