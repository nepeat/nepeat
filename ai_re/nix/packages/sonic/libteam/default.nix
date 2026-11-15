# libteam/teamd/teamdctl with SONiC's patch series, which teammgrd relies on
# (teamd -w/-o warm reboot, LACP "fallback", admin-down member handling, ...).
# Upstream recipe: src/libteam/Makefile + rules/libteam.mk in sonic-buildimage:
# Debian's libteam 1.31-1 (upstream 1.31, no Debian patches) + the active
# lines of src/libteam/patch/series. nixpkgs has no libteam.
# Deviations: built from the upstream v1.31 tag (autoreconf instead of the
# release tarball's configure); D-Bus and ZeroMQ control APIs off (Debian has
# D-Bus on); teamdctl and teammgrd talk to teamd over its unix socket.
{ lib, stdenv, fetchFromGitHub, autoreconfHook, pkg-config, libnl, libdaemon, jansson, sonic }:
stdenv.mkDerivation {
  pname = "libteam-sonic";
  version = "1.31";            # LIBTEAM_VERSION in rules/libteam.mk
  src = fetchFromGitHub {
    owner = "jpirko";
    repo = "libteam";
    rev = "v1.31";
    hash = "sha256-V1l0e84G7uRPEyw6BOOsd3MJ/HqMSkJ9HckpMP7LJ7s=";
  };

  # SONiC's series, applied from the pinned buildimage tree (no copies here).
  postPatch = ''
    dir=${sonic.sources.buildimage}/src/libteam/patch
    for p in $(grep -v '^#' $dir/series); do
      echo "applying SONiC libteam patch $p"
      patch -p1 --no-backup-if-mismatch < $dir/$p
    done
  '';

  nativeBuildInputs = [ autoreconfHook pkg-config ];
  buildInputs = [ libnl libdaemon jansson ];
  configureFlags = [ "--disable-static" "--disable-dbus" "--disable-zmq" ];
  # AC_FUNC_MALLOC assumes malloc(0) == NULL when cross-compiling and swaps in rpl_malloc.
  env = lib.optionalAttrs (stdenv.hostPlatform != stdenv.buildPlatform) {
    ac_cv_func_malloc_0_nonnull = "yes";
  };
  enableParallelBuilding = true;

  meta = {
    description = "Team network device library, teamd and teamdctl, with SONiC's patches";
    homepage = "https://github.com/jpirko/libteam";
    license = lib.licenses.lgpl21Plus;
    platforms = lib.platforms.linux;
  };
}
