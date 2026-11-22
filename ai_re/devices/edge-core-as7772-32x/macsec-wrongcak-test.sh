#!/usr/bin/env bash
# Run ON the AS7772 (root, login shell), after macsec-traffic-test.sh: one end of cage 1 <-> 2 gets a profile with
# the same CKN and another CAK. Expected: no MKA, no SAs, no data traffic on that link (EAPOL still passes); the
# other link is unaffected; with the right profile back the link returns. No key is printed.
set -u
export SONIC_CLI_IFACE_MODE=alias
D() { openbcm-diag "$@"; }
st() { D extmacsec | grep -E '^ce(0|1|30) ' | awk '{print $1, $3, "gate", $5, "SAs", $6 $7 $8 $9}' | tr '\n' ';'; echo; }
pings() { printf 'cage 1 -> 2: '; ping -q -c 100 -i 0.01 -W 1 10.0.1.1 | grep loss; printf 'cage 31 -> 32: '; ping -q -c 100 -i 0.01 -W 1 10.0.31.1 | grep loss; }
exec_start=$(systemctl show -p ExecStart --value sonic-macsec-config | grep -o 'argv\[\]=[^;]*' | cut -d= -f2)
py=$(echo $exec_start | awk '{print $1}'); script=$(echo $exec_start | awk '{print $3}'); decl=$(echo $exec_start | awk '{print $4}')
echo "== $(date -Is) before"; st; pings
umask 077; head -c 32 /dev/urandom | od -An -v -tx1 | tr -d ' \n' > /var/lib/sonic-macsec/wrong-test.cak
$py - "$decl" > /run/macsec-wrong.json <<'EOF'
import json, sys
d = json.load(open(sys.argv[1]))
d["profiles"]["wrong-test"] = dict(d["profiles"]["lab-test"], cakFile="/var/lib/sonic-macsec/wrong-test.cak")
d["ports"]["Ethernet4"] = "wrong-test"
d["stateFile"] = "/run/macsec-wrong-state.json"
print(json.dumps(d))
EOF
echo "== cage 2 (Ethernet4) -> profile wrong-test (same CKN, other CAK)"
$py -I $script /run/macsec-wrong.json
sleep 20
st; pings
journalctl -u sonic-macsecmgrd --since -20s --no-pager | grep -i -E 'ICV|not match|mismatch|CKN|invalid' | sed 's/^.*wpa_supplicant\[[0-9]*\]: //' | sort | uniq -c | head -5
show macsec --mka 2>&1 | grep -v -i -E 'sak|auth_key|salt' | head -30
echo "== the declared profile again"
systemctl restart sonic-macsec-config; sleep 1
sonic-db-cli CONFIG_DB del 'MACSEC_PROFILE|wrong-test' >/dev/null; rm -f /var/lib/sonic-macsec/wrong-test.cak /run/macsec-wrong.json /run/macsec-wrong-state.json
for i in $(seq 60); do [ "$(D extmacsec | grep -c ' open ')" = 8 ] && break; sleep 1; done
echo "MKA up after $i s"; st; ping -c 2 -W 1 10.0.1.1 >/dev/null; pings
echo "== failed units: $(systemctl --failed --no-legend | wc -l); profiles: $(sonic-db-cli CONFIG_DB keys 'MACSEC_PROFILE|*' | tr '\n' ' ')"
