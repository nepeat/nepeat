#!/usr/bin/env bash
set -euo pipefail
build_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
if [[ ${OPENWRT_FHS:-0} != 1 ]]; then
    exec openwrt-build-shell --noprofile --norc "$build_root/build.sh" "$@"
fi
export PATH="$build_root/host-tools:/usr/bin:/usr/sbin:/bin:/sbin"
# The FHS user namespace only maps the invoking user's IDs. Let fakeroot
# record package ownership without attempting chown to unmapped IDs.
export FAKEROOTDONTTRYCHOWN=1
unset C_INCLUDE_PATH CPLUS_INCLUDE_PATH LIBRARY_PATH HA_KEY HA_BASE
python3 "$build_root/prepare.py"
cd "$build_root/work/source"
./scripts/feeds update packages routing luci
./scripts/feeds install -a -p packages
./scripts/feeds install -a -p routing
./scripts/feeds install -a -p luci
# Refresh the seed after feed indexing so interactive edits made during setup
# are represented consistently in the generated configuration.
python3 "$build_root/prepare.py"
make defconfig
jobs="${OPENWRT_JOBS:-16}"
make -j"$jobs" download
make -j"$jobs" V=s
python3 "$build_root/package_netboot.py"
