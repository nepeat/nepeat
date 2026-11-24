#!/usr/bin/env bash
# Run ON the AS7772 (root, login shell), SONiC up with MACsec (services.sonic.macsec): one end of cage 1 <-> 2 gets a
# profile with the same CKN and another CAK. Expected: no MKA, no SAs, no data traffic on that link (EAPOL still
# passes); the other link is unaffected; with the right profile back the link returns. No key is printed.
#   bash -l macsec-wrongcak-test.sh [--dry-run]            the test; --dry-run prints the writes, makes none
#   bash -l macsec-wrongcak-test.sh --cleanup [--dry-run]  undo an interrupted run
# Builds the VRF-pair topology of macsec-traffic-test.sh itself (loadtest/sonic-test-lib.sh beside it or in
# ./loadtest/: CONFIG_DB snapshot, restore on every exit, the whole CONFIG_DB compared with it, refuses over an
# earlier run's leftovers). The wrong profile is written by sonic-macsec-config's own script from a copy of the
# declaration (key file, declaration and state file in the snapshot directory, mode 700, removed with it); the
# right one comes back from the snapshot (PORT|Ethernet4 macsec, MACSEC_PROFILE|wrong-test deleted).
set -u
here=$(cd "$(dirname "$0")" && pwd)
lib=
for l in "${SONIC_TEST_LIB:-}" "$here/loadtest/sonic-test-lib.sh" "$here/sonic-test-lib.sh"; do
  if [ -n "$l" ] && [ -r "$l" ]; then lib=$l; break; fi
done
[ -n "$lib" ] || { echo "sonic-test-lib.sh not found: copy loadtest/sonic-test-lib.sh next to this script"; exit 2; }
# shellcheck source=loadtest/sonic-test-lib.sh
. "$lib"
st_args "$@"
OWN=('^PORT[|]Ethernet4$' '^MACSEC_PROFILE[|]wrong-test$')
ST_PHASES=("${OWN[@]}" "${ST_PAIRS_PHASES[@]}")
export SONIC_CLI_IFACE_MODE=alias
D() { openbcm-diag "$@"; }
st() { D extmacsec | grep -E '^ce(0|1|30) ' | awk '{print $1, $3, "gate", $5, "SAs", $6 $7 $8 $9}' | tr '\n' ';'; echo; }
pings() { printf 'cage 1 -> 2: '; ping -q -c 100 -i 0.01 -W 1 10.0.1.1 | grep loss; printf 'cage 31 -> 32: '; ping -q -c 100 -i 0.01 -W 1 10.0.31.1 | grep loss; }
st_verify_extra() { st_mka_verify; }
# --cleanup without a snapshot (an older version interrupted): no wrong-test profile, Ethernet4 on Ethernet0's profile
st_synth_configdb() {
  st_pairs_synth_configdb | awk -F'\t' -v p="$(st_hget 'PORT|Ethernet0' macsec)" 'BEGIN { OFS = FS }
    $1 == "MACSEC_PROFILE|wrong-test" { next }
    $1 == "PORT|Ethernet4" && $2 == "macsec" && p != "" { $3 = p }
    { print }'
}
[ "$ST_MODE" = cleanup ] && st_cleanup macsec-wrongcak-test
read -r py _ script decl _ <<< "$(systemctl show -p ExecStart --value sonic-macsec-config | grep -o 'argv\[\]=[^;]*' | cut -d= -f2)"
prof=$(st_hget 'PORT|Ethernet0' macsec)
echo "== $(date -Is) $(readlink /run/current-system)"
st_pairs_macsec_check
[ -n "$prof" ] || st_refuse "CONFIG_DB PORT|Ethernet0 has no MACsec profile"
[ "$(st_hget 'PORT|Ethernet4' macsec)" = "$prof" ] || st_refuse "CONFIG_DB PORT|Ethernet4 macsec is '$(st_hget 'PORT|Ethernet4' macsec)', not Ethernet0's '$prof' (--cleanup)"
st_has "$CDB" 'MACSEC_PROFILE|wrong-test' && st_refuse "CONFIG_DB MACSEC_PROFILE|wrong-test exists (--cleanup)"
if ! command -v "$py" > /dev/null || [ ! -r "$script" ] || [ ! -r "$decl" ]; then
  st_refuse "sonic-macsec-config's ExecStart not understood (python '$py', script '$script', declaration '$decl')"
fi
st_refusals || exit 1
st_pairs_session macsec-wrongcak-test
w=$ST_DIR/wrong-test
if [ "$ST_DRY" = 0 ]; then echo "== $(date -Is) before"; st; pings; fi
# the declaration with profile wrong-test (the right one's fields, a random key of its length) on Ethernet4
"$py" - "$decl" "$prof" "$w" "$ST_DRY" > "$w.json" <<'EOF' || { echo "FAIL: no declaration for the wrong profile"; exit 1; }
import json, os, sys
decl, prof, w, dry = sys.argv[1:]
d = json.load(open(decl))
p = d["profiles"][prof]
d["profiles"]["wrong-test"] = dict(p, cakFile=w + ".cak")
d["ports"]["Ethernet4"] = "wrong-test"
d["stateFile"] = w + "-state.json"
if dry == "0":
    fd = os.open(w + ".cak", os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    os.write(fd, os.urandom(p["cakHexDigits"] // 2).hex().encode() + b"\n")
    os.close(fd)
print(json.dumps(d))
EOF
echo "== cage 2 (Ethernet4) -> profile wrong-test (same CKN, other CAK)"
st_x "$py" -I "$script" "$w.json"
[ "$ST_DRY" = 1 ] && exit 0
sleep 20
st; pings
journalctl -u sonic-macsecmgrd --since -20s --no-pager | grep -i -E 'ICV|not match|mismatch|CKN|invalid' | sed 's/^.*wpa_supplicant\[[0-9]*\]: //' | sort | uniq -c | head -5
show macsec --mka 2>&1 | grep -v -i -E 'sak|auth_key|salt' | head -30
echo "== the declared profile again (from the snapshot: PORT|Ethernet4 macsec, MACSEC_PROFILE|wrong-test)"
st_restore_configdb "$ST_DIR/configdb-before.tsv" "${OWN[@]}" || exit 1
rc=0
if st_mka 60; then echo "MKA up after $ST_MKA_S s"; else echo "FAIL: MKA not back 60 s after the right profile"; rc=1; fi
st; ping -c 2 -W 1 10.0.1.1 >/dev/null; pings
echo "== failed units: $(systemctl --failed --no-legend | wc -l); profiles: $(st_r "$CDB" keys 'MACSEC_PROFILE|*' | tr '\n' ' ')"
exit $rc
