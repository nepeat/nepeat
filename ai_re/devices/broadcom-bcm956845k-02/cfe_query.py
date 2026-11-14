import argparse
import re
import subprocess
import time

parser = argparse.ArgumentParser()
parser.add_argument('command')
parser.add_argument('--timeout', type=float, default=60)
args = parser.parse_args()
session = 'serial-bcm956845'
def capture():
    return subprocess.check_output(['tmux', 'capture-pane', '-t', session, '-p', '-J', '-S', '-10000']).decode(errors='replace')
marker = 'CFE> ' + args.command + '\n'
previous = capture().count(marker)
subprocess.run(['tmux', 'send-keys', '-t', session, '-l', args.command], check=True)
subprocess.run(['tmux', 'send-keys', '-t', session, 'Enter'], check=True)
text = capture()
start = time.monotonic()
while time.monotonic() - start < args.timeout:
    text = capture()
    index = text.rfind(marker)
    if text.count(marker) > previous:
        result = text[index:].rstrip()
        if result.endswith('CFE>'):
            print(result, flush=True)
            statuses = re.findall(r'\*\*\* command status = (-?\d+)', result)
            if statuses and int(statuses[-1]) != 0:
                raise SystemExit(f'CFE command failed with status {statuses[-1]}')
            break
    time.sleep(.2)
else:
    print(text[-6000:], flush=True)
    raise SystemExit('Timed out waiting for CFE prompt; inspect console before next command')
