include $(TOPDIR)/rules.mk
include $(INCLUDE_DIR)/kernel.mk

PKG_NAME:=bcm-bde-compat
PKG_RELEASE:=1
PKG_LICENSE:=GPL-2.0-only
PKG_BUILD_DIR:=$(KERNEL_BUILD_DIR)/$(PKG_NAME)

include $(INCLUDE_DIR)/package.mk

define KernelPackage/bcm-bde-compat
  SUBMENU:=Network Devices
  TITLE:=BCM56845 legacy userspace SDK compatibility
  DEPENDS:=@TARGET_mpc85xx_bcm98548 @PCI_SUPPORT
  FILES:=$(PKG_BUILD_DIR)/bcm-bde-compat.ko
endef

define Build/Prepare
	mkdir -p $(PKG_BUILD_DIR)
	$(CP) ./src/* $(PKG_BUILD_DIR)/
endef

define Build/Compile
	+$(KERNEL_MAKE) M="$(PKG_BUILD_DIR)" modules
endef

$(eval $(call KernelPackage,bcm-bde-compat))
