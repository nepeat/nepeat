# Product definition for Amazon "yacht" / KFYAWI TWRP build.
$(call inherit-product, $(SRC_TARGET_DIR)/product/embedded.mk)
$(call inherit-product, $(SRC_TARGET_DIR)/product/languages_full.mk)
$(call inherit-product, vendor/omni/config/common.mk)

PRODUCT_DEVICE       := yacht
PRODUCT_NAME         := omni_yacht
PRODUCT_BRAND        := Amazon
PRODUCT_MODEL        := KFYAWI
PRODUCT_MANUFACTURER := Amazon

# Android 9 / Fire OS 7.4.0.1
PRODUCT_TARGET_VNDK_VERSION := 28

PRODUCT_BUILD_PROP_OVERRIDES += \
    TARGET_DEVICE=yacht \
    PRODUCT_NAME=yacht
