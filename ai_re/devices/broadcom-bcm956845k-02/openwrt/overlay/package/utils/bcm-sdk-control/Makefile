include $(TOPDIR)/rules.mk

PKG_NAME:=bcm-sdk-control
PKG_RELEASE:=1
PKG_LICENSE:=GPL-2.0-only
PKG_BUILD_DIR:=$(BUILD_DIR)/$(PKG_NAME)

include $(INCLUDE_DIR)/package.mk

define Package/bcm-sdk-control
  SECTION:=net
  CATEGORY:=Network
  TITLE:=BCM56845 OpenWrt control service and LuCI integration
  DEPENDS:=@TARGET_mpc85xx_bcm98548 +kmod-bcm-bde-compat +kmod-tun +jshn +jsonfilter
endef

define Package/bcm-sdk-control/conffiles
/etc/config/bcm_switch
endef

define Build/Prepare
	mkdir -p $(PKG_BUILD_DIR)
	$(CP) ./src/* $(PKG_BUILD_DIR)/
endef

define Build/Compile
	$(TARGET_CC) $(TARGET_CFLAGS) -Wall -Wextra $(TARGET_LDFLAGS) \
		-o $(PKG_BUILD_DIR)/bcm-control $(PKG_BUILD_DIR)/bcm-control.c -lutil
endef

define Package/bcm-sdk-control/install
	$(INSTALL_DIR) $(1)/usr/sbin
	$(INSTALL_BIN) $(PKG_BUILD_DIR)/bcm-control $(1)/usr/sbin/bcmd
	$(LN) bcmd $(1)/usr/sbin/bcmctl
	$(CP) ./files/. $(1)/
endef

$(eval $(call BuildPackage,bcm-sdk-control))
