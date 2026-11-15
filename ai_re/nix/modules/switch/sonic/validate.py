"""Build-time YANG validation of the Nix base config (sonic-yang-mgmt).
usage: validate.py YANG_DIR CONFIG_DB_JSON [TABLE_ALLOWED_WITHOUT_YANG ...]"""
import json, sys
import sonic_yang

yang_dir, path, allowed = sys.argv[1], sys.argv[2], set(sys.argv[3:])
with open(path) as f:
    cfg = json.load(f)
sy = sonic_yang.SonicYang(yang_dir, print_log_enabled=False)
sy.loadYangModel()
try:
    sy.loadData(configdbJson=cfg)
    sy.validate_data_tree()
except sonic_yang.SonicYangException as e:
    sys.exit("services.sonic: the Nix base config fails YANG validation: %s" % e)
missing = sorted(set(sy.tablesWithOutYang) - allowed - {"bgpraw"})
if missing:
    sys.exit("services.sonic: tables without a YANG model (add to tablesWithoutYang to allow): " + ", ".join(missing))
print("YANG OK: %d tables, %d keys" % (len(cfg), sum(len(v) for v in cfg.values())))
