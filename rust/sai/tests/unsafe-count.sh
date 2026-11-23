#!/usr/bin/env bash
# Unsafe inventory of rust/sai (README "Unsafe"): per file of crates/*/src, the `unsafe {` blocks
# before the file's #[cfg(test)] module (comment lines excluded), after it, and in tests/;
# `safe fn` declarations and #[unsafe(no_mangle)] exports.
cd "${1:-.}"
count() { awk -v mode="$2" '
  /^#\[cfg\(test\)\]/ { intest = 1 }
  /^[[:space:]]*\/\// { next }
  { n = gsub(/unsafe \{/, "&"); if (intest) t += n; else s += n }
  END { if (mode == "src") print s + 0; else print t + 0 }' "$1"; }
printf '%-22s %-26s %5s %5s %5s %5s\n' crate file src intest safefn export
for f in $(find crates -path '*/src/*' -name '*.rs' | sort); do
  c=$(echo "$f" | cut -d/ -f2); s=$(count "$f" src); t=$(count "$f" test)
  sf=$(grep -c '^[[:space:]]*\(pub\(([a-z]*)\)\? \)\?safe fn' "$f"); ex=$(grep -c 'unsafe(no_mangle)' "$f")
  [ "$s$t$sf$ex" = "0000" ] || printf '%-22s %-26s %5s %5s %5s %5s\n' "$c" "${f#crates/$c/src/}" "$s" "$t" "$sf" "$ex"
done
echo "== tests/ dirs (unsafe { blocks, comment lines excluded)"
for d in crates/*/tests; do n=$(cat $(find "$d" -name '*.rs') /dev/null | grep -v '^[[:space:]]*//' | grep -o 'unsafe {' | wc -l); [ "$n" = 0 ] || echo "$d $n"; done
