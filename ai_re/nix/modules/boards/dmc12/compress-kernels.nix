# gzip the raw kernels nixpkgs' extlinux builder copies to /boot/nixos (48 MB lean -> ~16 MB).
# Our U-Boot's booti decompresses them itself (gzip magic; kernel_comp_addr_r/kernel_comp_size in
# its env, SYS_BOOTM_LEN 256 MiB). In place and idempotent: names stay, so the builders' own
# bookkeeping (skip-if-present, clean-up by name) is unaffected. Not for the factory U-Boot's
# uImages (/boot/nixos-uboot): its gunzip limit is unknown.
{ pkgs }:
pkgs.writeShellScript "dmc12-gzip-boot-kernels" ''
  set -e
  export PATH=${pkgs.lib.makeBinPath [ pkgs.coreutils pkgs.gzip ]}
  for f in "''${1:-/boot}"/nixos/*-Image; do
    [ -f "$f" ] || continue
    [ "$(head -c 2 "$f" | od -An -tx1 | tr -d ' ')" = "1f8b" ] && continue
    gzip -9n -c "$f" > "$f.gz.tmp"
    mv -f "$f.gz.tmp" "$f"
  done
''
