# Product definition for Amazon "yacht" / KFYAWI TWRP build.
$(call inherit-product, $(SRC_TARGET_DIR)/product/embedded.mk)
$(call inherit-product, $(SRC_TARGET_DIR)/product/languages_full.mk)
$(call inherit-product, vendor/twrp/config/common.mk)

PRODUCT_DEVICE       := yacht
PRODUCT_NAME         := twrp_yacht
PRODUCT_BRAND        := Amazon
PRODUCT_MODEL        := KFYAWI
PRODUCT_MANUFACTURER := Amazon

PRODUCT_TARGET_VNDK_VERSION := 28   # Android 9 / Fire OS 7.4.0.1

PRODUCT_BUILD_PROP_OVERRIDES += \
    TARGET_DEVICE=yacht \
    PRODUCT_NAME=yacht

# Matches the stock fingerprint recorded from the device:
# Amazon/yacht/pinnacles:9/PS7401.3594N/0025535842816:user/amz-p,release-keys
