
import ast, sys
src = open(sys.argv[1]).read()
tree = ast.parse(src)
want = ["XCVR_IDENTIFIERS","XCVR_IDENTIFIER_ABBRV","CONNECTORS","MODULE_MEDIA_TYPE","HOST_ELECTRICAL_INTERFACE","NM_850_MEDIA_INTERFACE","SM_MEDIA_INTERFACE","PASSIVE_COPPER_MEDIA_INTERFACE","ACTIVE_CABLE_MEDIA_INTERFACE","BASE_T_MEDIA_INTERFACE"]
out = []
for node in ast.walk(tree):
    if isinstance(node, ast.Assign) and len(node.targets)==1 and isinstance(node.targets[0], ast.Name) and node.targets[0].id in want:
        d = ast.literal_eval(node.value)
        name = node.targets[0].id
        out.append((name, d))
out.sort(key=lambda x: want.index(x[0]))
print("// Generated from sonic-platform-common sonic_xcvr/codes/public/sff8024.py (SFF-8024 code tables);")
print("// regenerate with the script in README.md \"Code tables\". Do not edit by hand.")
for name, d in out:
    print(f"/// SFF-8024 {name.replace('_',' ').lower()}.")
    print(f"pub(crate) const {name}: &[(u8, &str)] = &[")
    for k in sorted(d):
        v = d[k].replace('\\','\\\\').replace('"','\\"')
        print(f'    ({k:#04x}, "{v}"),')
    print("];")
