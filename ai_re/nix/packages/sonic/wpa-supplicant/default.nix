# sonic-wpa-supplicant: hostap with SONiC's MACsec driver (-D macsec_sonic).
# The KaY (MKA key agreement, XPN cipher suites, rekey period) runs here; the
# driver writes what it negotiates into APPL_DB (MACSEC_PORT / _SC / _SA
# tables, which orchagent's MACsecOrch turns into SAI MACsec objects) and
# reads packet numbers from COUNTERS_DB. Started per port by swss's
# macsecmgrd. Revision: the sonic-buildimage pin's src/wpasupplicant
# submodule (sources.json).
# Only what MKA with a pre-shared CAK on a wired port needs is built: no
# Wi-Fi, no EAP methods, no D-Bus (upstream's Debian config builds everything).
{ lib, stdenv, pkg-config, openssl, hiredis, libnl, boost, nlohmann_json, libuuid, sonic }:
let
  inherit (sonic) swss-common;
  dotConfig = ''
    CONFIG_DRIVER_WIRED=y
    CONFIG_DRIVER_MACSEC_SONIC=y
    CONFIG_IEEE8021X_EAPOL=y
    CONFIG_MACSEC=y
    CONFIG_CTRL_IFACE=y
    CONFIG_BACKEND=file
    CONFIG_MAIN=main
    CONFIG_OS=unix
    CONFIG_ELOOP=eloop
    CONFIG_L2_PACKET=linux
    CONFIG_TLS=openssl
    CONFIG_DEBUG_SYSLOG=y
    CONFIG_DEBUG_SYSLOG_FACILITY=LOG_DAEMON
  '';
in stdenv.mkDerivation {
  pname = "sonic-wpa-supplicant";
  version = sonic.version "wpa-supplicant";
  src = sonic.sources.wpa-supplicant;

  nativeBuildInputs = [ pkg-config ];
  buildInputs = [ openssl swss-common hiredis libnl boost nlohmann_json libuuid ];   # the last three: swss-common's headers

  postPatch = ''
    printf '%s' ${lib.escapeShellArg dotConfig} > wpa_supplicant/.config
  '';
  # The driver's C++ half (src/drivers/sonic_operators.cpp) goes through the
  # same $(CC) rule as the C files.
  env.NIX_CFLAGS_COMPILE = toString [ "-I${swss-common}/include" "-Wno-deprecated-declarations" ];
  makeFlags = [ "-C" "wpa_supplicant" "CC=${stdenv.cc.targetPrefix}cc" "V=1" ];
  buildFlags = [ "wpa_supplicant" "wpa_cli" ];
  enableParallelBuilding = true;
  installPhase = ''
    runHook preInstall
    install -Dm755 wpa_supplicant/wpa_supplicant wpa_supplicant/wpa_cli -t $out/bin
    runHook postInstall
  '';

  meta = {
    description = "wpa_supplicant with SONiC's MACsec driver (MKA for SONiC MACsec)";
    homepage = "https://github.com/sonic-net/sonic-wpa-supplicant";
    license = lib.licenses.bsd3;
    platforms = lib.platforms.linux;
    mainProgram = "wpa_supplicant";
  };
}
