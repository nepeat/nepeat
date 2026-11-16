"""cmx_port_reset must close both directions and drop every SA."""
import importlib.util
import sys

spec = importlib.util.spec_from_file_location('cm', sys.argv[1])
cm = importlib.util.module_from_spec(spec)
spec.loader.exec_module(cm)
cm.Phy().cmd('cmx_port_reset', slice=16)
st = cm.collect(16)[16]
assert st['tx_drop'] == 1 and st['rx_drop'] == 1 and not st['sa'], st
print('reset ok: slice 16 blocked, no SAs')
