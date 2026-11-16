# qemu -d out_asm without a disassembler prints host code as little-endian hex
# ("OBJD-H: ..."): count aarch64 LDXP/STXP (64-bit pair) and DMB ISH*/OSH* words.
/^OBJD-H:/ {
  s = $2
  for (i = 1; i + 7 <= length(s); i += 8) {
    x = strtonum("0x" substr(s, i + 6, 2) substr(s, i + 4, 2) substr(s, i + 2, 2) substr(s, i, 2))
    if (and(x, 0xffff8000) == 0xc87f0000) ldxp++
    if (and(x, 0xffe08000) == 0xc8200000) stxp++
    if (and(x, 0xfffffcff) == 0xd50338bf) ish++
    if (and(x, 0xfffffcff) == 0xd50330bf) osh++
  }
}
END { printf "ldxp=%d stxp=%d dmb_ish=%d dmb_osh=%d\n", ldxp, stxp, ish, osh }
