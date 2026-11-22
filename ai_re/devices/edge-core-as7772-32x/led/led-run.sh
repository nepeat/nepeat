#!/usr/bin/env bash
# Run ON the AS7772 (root) with SONiC up: feed a .soc file of LED commands to the SDK diag shell inside
# syncd, one line per openbcm-diag call (the socket refuses rcload and ";"). Comments and empty lines are
# skipped; only "led ..." lines are accepted.
#   led-run.sh pattern-blue.soc
#   led-run.sh normal          # back to the system's own LED program (the rc file syncd loaded at start)
set -euo pipefail
f=${1:?usage: led-run.sh file.soc | normal}
if [ "$f" = normal ]; then
  profile=$(systemctl cat sonic-syncd | grep -o '/nix/store/[^ ]*-sai.profile' | head -1)
  f=$(sed -n 's/^SAI_OPENBCM_RC_FILE=//p' "$profile")
  [ -n "$f" ] || { echo "this system has no LED program (no SAI_OPENBCM_RC_FILE in $profile)"; exit 1; }
fi
bad=$(grep -vE '^[[:space:]]*(#.*)?$|^led ' "$f" || true)
[ -z "$bad" ] || { echo "refusing, not LED commands:"; echo "$bad"; exit 1; }
grep -E '^led ' "$f" | while IFS= read -r line; do
  echo "+ ${line:0:60}"
  openbcm-diag "$line"
done
openbcm-diag "led 0 status"
openbcm-diag "led 1 status"
