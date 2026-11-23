# trial-boot: stage a one-shot trial boot, inspect, cancel, promote (./default.nix).
# Preamble (from Nix): FLAG_DEVICE FLAG_LBA BOOT_SCR SCRIPT_DEST CONF_FILE DTB_NAME
# APPEND STATE, loader_check(), loader_setup().

die() { echo "trial-boot: $*" >&2; exit 1; }
say() { echo "trial-boot: $*"; logger -t trial-boot -- "$*" 2>/dev/null || true; }
need_root() { [ "$(id -u)" = 0 ] || die "run as root"; }

# The request sector must lie before every partition of FLAG_DEVICE.
check_sector() {
  [ -b "$FLAG_DEVICE" ] || die "$FLAG_DEVICE is not a block device"
  local first
  first=$(partx -g -o START "$FLAG_DEVICE" | sort -n | head -n1 | tr -d ' ')
  [ -n "$first" ] || die "no partition table on $FLAG_DEVICE"
  [ "$FLAG_LBA" -lt "$first" ] || die "sector $FLAG_LBA is not before the first partition ($first)"
  [ "$FLAG_LBA" -ge 1 ] || die "sector 0 is the partition table"
}
# prints: idle | pending | foreign
sector_state() {
  local hex
  hex=$(dd if="$FLAG_DEVICE" bs=512 skip="$FLAG_LBA" count=1 iflag=direct status=none | od -An -v -tx1 | tr -d ' \n')
  if [ "$hex" = "$(printf '%01024d' 0)" ]; then echo idle
  elif [ "${hex:0:16}" = 545249414c425431 ]; then echo pending   # "TRIALBT1"
  else echo foreign; fi
}
write_sector() {   # $1: file with exactly 512 bytes
  [ "$(stat -c %s "$1")" = 512 ] || die "internal: request is not 512 bytes"
  dd if="$1" of="$FLAG_DEVICE" bs=512 seek="$FLAG_LBA" count=1 conv=notrunc,fsync oflag=direct status=none
  blockdev --flushbufs "$FLAG_DEVICE" 2>/dev/null || true
}
cmdline_nonce() { sed -n 's/.*\btrialboot=\([0-9a-f]*\).*/\1/p' /proc/cmdline; }

cmd_setup() {
  need_root
  check_sector
  mkdir -p "$STATE" "$(dirname "$CONF_FILE")"
  if ! cmp -s "$BOOT_SCR" "$SCRIPT_DEST"; then
    cp "$BOOT_SCR" "$SCRIPT_DEST.tmp" && sync "$SCRIPT_DEST.tmp" && mv "$SCRIPT_DEST.tmp" "$SCRIPT_DEST" && sync
    say "installed $SCRIPT_DEST ($(sha256sum "$SCRIPT_DEST" | cut -c1-16))"
  else
    say "$SCRIPT_DEST is current"
  fi
  if loader_check; then say "loader runs the script first: ok"
  else loader_setup; loader_check || die "loader setting did not take"; say "loader setting made"; fi
}

cmd_stage() {
  need_root
  local top="" extra="" reboot=0 force=0
  while [ $# -gt 0 ]; do
    case $1 in
      --append) extra="$extra ${2:?--append needs an argument}"; shift 2 ;;
      --reboot) reboot=1; shift ;;
      --force) force=1; shift ;;
      -*) die "unknown option $1" ;;
      *) [ -z "$top" ] || die "one toplevel only"; top=$1; shift ;;
    esac
  done
  [ -n "$top" ] || die "usage: trial-boot stage <toplevel> [--append ARGS] [--reboot] [--force]"
  top=$(readlink -f "$top")
  case $top in /nix/store/*) ;; *) die "$top is not a store path" ;; esac
  for f in kernel initrd init kernel-params; do [ -e "$top/$f" ] || die "$top has no $f"; done
  check_sector
  cmp -s "$BOOT_SCR" "$SCRIPT_DEST" || die "$SCRIPT_DEST is missing or differs: run 'trial-boot setup' (from this trial) first"
  loader_check || die "the loader does not run the script before extlinux: run 'trial-boot setup'"
  local st; st=$(sector_state)
  [ "$st" != foreign ] || die "sector $FLAG_LBA of $FLAG_DEVICE holds something else: not touching it"
  [ "$st" = idle ] || [ $force = 1 ] || die "a trial is already pending (trial-boot cancel, or --force)"

  local kernel initrd dtb="" nonce
  kernel=$(readlink -f "$top/kernel"); initrd=$(readlink -f "$top/initrd")
  if [ -n "$DTB_NAME" ]; then dtb=$(readlink -f "$top/dtbs/$DTB_NAME"); [ -f "$dtb" ] || die "no $DTB_NAME in $top/dtbs"; fi
  nonce=$(od -An -N6 -tx1 /dev/urandom | tr -d ' \n')
  mkdir -p "$STATE" "$(dirname "$CONF_FILE")"
  # keep the trial (and so its kernel files) alive across a garbage collection
  ln -sfn "$top" /nix/var/nix/gcroots/trial-boot
  {
    echo "# trial-boot $nonce $(date -Is): one-shot trial, read by U-Boot from the root fs"
    echo "DEFAULT trial"
    echo "LABEL trial"
    echo "  MENU LABEL trial $nonce $(basename "$top")"
    echo "  LINUX $kernel"
    echo "  INITRD $initrd"
    [ -z "$dtb" ] || echo "  FDT $dtb"
    echo "  APPEND init=$top/init $(cat "$top/kernel-params") trialboot=$nonce $APPEND$extra"
  } > "$CONF_FILE.tmp"
  sync "$CONF_FILE.tmp"; mv "$CONF_FILE.tmp" "$CONF_FILE"; sync
  echo "$nonce $(date -Is) $top$extra" > "$STATE/staged"
  sync "$STATE/staged"
  local req; req=$(mktemp)
  { printf 'TRIALBT1'; printf ' %s %s\n' "$nonce" "$top"; } > "$req"
  truncate -s 512 "$req"
  write_sector "$req"; rm -f "$req"
  [ "$(sector_state)" = pending ] || die "request did not read back"
  say "staged trial $nonce: $top$extra (the next boot only; the default stays $(readlink -f /nix/var/nix/profiles/system))"
  if [ $reboot = 1 ]; then say "rebooting into the trial"; systemctl reboot; fi
}

cmd_cancel() {
  need_root; check_sector
  local st; st=$(sector_state)
  [ "$st" != foreign ] || die "sector $FLAG_LBA holds something else: not touching it"
  if [ "$st" = pending ]; then
    local z; z=$(mktemp); truncate -s 512 "$z"; write_sector "$z"; rm -f "$z"
    say "request cancelled"
  else say "no request pending"; fi
}

cmd_status() {
  local n; n=$(cmdline_nonce)
  if [ -n "$n" ]; then echo "this boot: trial $n ($(cat /run/trial-boot/state 2>/dev/null || echo verifying))"
  else echo "this boot: not a trial"; fi
  echo "request sector $FLAG_DEVICE:$FLAG_LBA: $(sector_state 2>/dev/null || echo unreadable)"
  echo "default (system profile): $(readlink -f /nix/var/nix/profiles/system)"
  [ ! -f "$STATE/staged" ] || echo "last staged: $(cat "$STATE/staged")"
  if [ -f "$STATE/history" ]; then echo "history (last 10):"; tail -n 10 "$STATE/history"; fi
}

cmd_promote() {
  need_root
  local n; n=$(cmdline_nonce)
  [ -n "$n" ] || die "this boot is not a trial: nothing to promote"
  [ "$(cat /run/trial-boot/state 2>/dev/null)" = verified ] || die "this trial has not passed its checks"
  local top; top=$(readlink -f /run/booted-system)
  nix-env -p /nix/var/nix/profiles/system --set "$top"
  "$top/bin/switch-to-configuration" boot
  echo "$n $(date -Is) promoted $top" >> "$STATE/history"
  say "promoted $top: it is the default from now on"
}

case ${1:-} in
  setup) shift; cmd_setup "$@" ;;
  stage) shift; cmd_stage "$@" ;;
  cancel) shift; cmd_cancel ;;
  status) shift; cmd_status ;;
  promote) shift; cmd_promote ;;
  *) echo "usage: trial-boot setup | stage <toplevel> [--append ARGS] [--reboot] [--force] | cancel | status | promote" >&2; exit 2 ;;
esac
