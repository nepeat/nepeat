define Device/broadcom_bcm98548xmc
  DEVICE_VENDOR := Broadcom
  DEVICE_MODEL := BCM98548XMC
  DEVICE_VARIANT := RAM netboot
  DEVICE_DTS := bcm98548xmc-netboot
  KERNEL := kernel-bin | gzip | uImage gzip
  KERNEL_INITRAMFS := kernel-bin | gzip | uImage gzip
  KERNEL_INITRAMFS_SUFFIX := -initramfs.uImage
  IMAGES :=
  DEVICE_PACKAGES :=
endef
TARGET_DEVICES += broadcom_bcm98548xmc
