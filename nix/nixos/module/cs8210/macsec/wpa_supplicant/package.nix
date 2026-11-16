# wpa_supplicant 2.12 with the CS8210 MACsec driver (-D macsec_cs8210),
# see driver_macsec_cs8210.c. callPackage'd by ../default.nix and by
# ../test/run-mka-test.sh (x86, against the fake phy_server).
{ wpa_supplicant }:

wpa_supplicant.overrideAttrs (old: {
  pname = "wpa_supplicant-cs8210";
  postPatch = (old.postPatch or "") + ''
    cp ${./driver_macsec_cs8210.c} src/drivers/driver_macsec_cs8210.c
    substituteInPlace src/drivers/driver.h --replace-fail \
      '#endif /* CONFIG_DRIVER_MACSEC_QCA */' \
      '#endif /* CONFIG_DRIVER_MACSEC_QCA */
    #ifdef CONFIG_DRIVER_MACSEC_CS8210
    extern const struct wpa_driver_ops wpa_driver_macsec_cs8210_ops;
    #endif /* CONFIG_DRIVER_MACSEC_CS8210 */'
    substituteInPlace src/drivers/drivers.c --replace-fail \
      '#endif /* CONFIG_DRIVER_MACSEC_QCA */' \
      '#endif /* CONFIG_DRIVER_MACSEC_QCA */
    #ifdef CONFIG_DRIVER_MACSEC_CS8210
      &wpa_driver_macsec_cs8210_ops,
    #endif /* CONFIG_DRIVER_MACSEC_CS8210 */'
    # before NEED_DRV_WIRED_COMMON: the end of drivers.mak expands DRV_OBJS
    # immediately (:=), so a block appended there would never be built
    substituteInPlace src/drivers/drivers.mak --replace-fail \
      'ifdef NEED_DRV_WIRED_COMMON' \
      'ifdef CONFIG_DRIVER_MACSEC_CS8210
    DRV_CFLAGS += -DCONFIG_DRIVER_MACSEC_CS8210
    DRV_OBJS += ../src/drivers/driver_macsec_cs8210.o
    NEED_DRV_WIRED_COMMON=1
    endif

    ifdef NEED_DRV_WIRED_COMMON'
  '';
  extraConfig = old.extraConfig + ''
    CONFIG_MACSEC=y
    CONFIG_DRIVER_MACSEC_CS8210=y
  '';
})
