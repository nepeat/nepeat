#!/usr/bin/env python3
"""Runs ON the AS7772 (root), SONiC stopped: one private OpenBCM diag shell (bcm.user) behind a unix socket that
speaks the openbcm-diag protocol (one command per connection, output back), so the ASIC ring commands, the
counter samples and the clause-45 traffic of the PHY re-host all go through ONE serialised SDK process:

  bcmd.py --config /nix/store/...-config.bcm [--socket /run/m-load/diag.sock] [--temp /run/asic/temperature]
  OPENBCM_DIAG_SOCKET=/run/m-load/diag.sock openbcm-diag 'show c nz ce0'

It runs 'init all' at start (MDIO rate/hold delay programmed: the "misc init before any PHY access" rule; the
PHYs are not touched by it) and publishes the hottest ASIC monitor every 5 s where syncd would, so fand keeps
following its normal curve during the test; if this process dies the file goes stale and fand fails safe.
'm-quit' exits the shell and the daemon. Runtime only: nothing here survives a reboot.
"""
import argparse, os, re, select, socket, subprocess, sys, time

ap = argparse.ArgumentParser()
ap.add_argument('--config', required=True)
ap.add_argument('--socket', default='/run/m-load/diag.sock')
ap.add_argument('--temp', default='/run/asic/temperature')
a = ap.parse_args()
d = os.path.dirname(a.socket)
os.makedirs(d, mode=0o700, exist_ok=True)
cfg = os.path.join(d, 'config.bcm')
if os.path.lexists(cfg):
    os.unlink(cfg)
os.symlink(a.config, cfg)
p = subprocess.Popen(['stdbuf', '-o0', '-e0', 'bcm.user'], cwd=d, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                     stderr=subprocess.STDOUT, bufsize=0, env=dict(os.environ, BCM_CONFIG_FILE=cfg))
buf, seq = b'', 0


def run(line, timeout=120):
    global buf, seq
    seq += 1
    mark = b'MK%d\n' % seq
    p.stdin.write(line.encode() + b'\necho ' + mark)
    end = time.time() + timeout
    while True:
        i = buf.find(mark)
        if i >= 0:
            out, buf = buf[:i], buf[i + len(mark):]
            return out
        r, _, _ = select.select([p.stdout], [], [], max(0.0, end - time.time()))
        if not r:
            return buf + b'\nbcmd: timeout\n'
        x = os.read(p.stdout.fileno(), 65536)
        if not x:
            raise SystemExit('bcmd: bcm.user exited: %r' % buf[-300:])
        buf += x


def temp():
    t = [float(x) for x in re.findall(rb'current=\s*([0-9.]+)', run('show temp'))]
    if t and a.temp:
        os.makedirs(os.path.dirname(a.temp), exist_ok=True)
        with open(a.temp + '.tmp', 'w') as f:
            f.write('%.1f\n' % max(t))
        os.rename(a.temp + '.tmp', a.temp)


banner = run('init all', 180)
sys.stdout.write(banner.decode('latin1')[-600:] + '\nbcmd: ready\n'); sys.stdout.flush()
if os.path.exists(a.socket):
    os.unlink(a.socket)
s = socket.socket(socket.AF_UNIX); s.bind(a.socket); os.chmod(a.socket, 0o600); s.listen(8)
last = 0
while True:
    if time.time() - last >= 5:
        temp(); last = time.time()
    r, _, _ = select.select([s], [], [], 1.0)
    if not r:
        continue
    c, _ = s.accept()
    try:
        c.settimeout(5)
        cmd = b''
        while True:
            x = c.recv(4096)
            if not x:
                break
            cmd += x
        cmd = cmd.decode().strip()
        if cmd == 'm-quit':
            c.sendall(b'bcmd: exiting\n'); c.close()
            p.stdin.write(b'exit\n'); p.wait(20); os.unlink(a.socket)
            break
        if '\n' in cmd or not cmd:
            c.sendall(b'openbcm-diag: one line per connection\n')
        elif cmd.split()[0].lower() in ('exit', 'quit', 'shell', 'sh', 'reset', 'rcload') or cmd[0] == '!' or ';' in cmd:
            c.sendall(b'openbcm-diag: refused (use m-quit to end the daemon)\n')   # as syncd's socket does
        else:
            c.sendall(run(cmd))
    except Exception as e:
        sys.stderr.write('bcmd: %r\n' % (e,))
    finally:
        try:
            c.close()
        except Exception:
            pass
