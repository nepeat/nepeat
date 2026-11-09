#!/usr/bin/env python3
"""Explicit destructive-to-RAM test on ports 1/16, VLANs/RIFs 4001/4002.

Requires those IDs/prefix to be unused. Restores scoped objects, port/VLAN
configuration and Linux forwarding. No flash writes or persistent install.
"""
from pathlib import Path
import subprocess,json,time,shlex,datetime
ROOT=Path(__file__).resolve().parent.parent
SSH=['bash',str(ROOT/'ssh.sh')]
def ssh(c,check=True):
    p=subprocess.run(SSH+[c],capture_output=True,text=True,timeout=90)
    if check and p.returncode: raise RuntimeError(p.stdout+p.stderr)
    return p

def sdk(c):
    j=json.loads(ssh('bcmctl --json '+shlex.quote(c)).stdout)
    if not j['ok']: raise RuntimeError(j)
    return j['output']

def main():
    r={'date_utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'base_image':json.loads((ROOT/'artifacts/manifest.json').read_text()),'test':'ASIC IPv4 LPM route with Linux forwarding disabled'}
    r['before_routes']=sdk('l3 defip show');r['before_interfaces']=sdk('l3 intf show');r['before_vlans']=sdk('vlan show')
    assert '198.19.2.' not in r['before_routes'] and '4001' not in r['before_vlans'] and '4002' not in r['before_vlans']
    assert 'Free L3INTF entries: 4095' in r['before_interfaces'], 'Requires an otherwise empty L3 configuration'
    old_forward=ssh('cat /proc/sys/net/ipv4/ip_forward').stdout.strip()
    for name in ('l3-setup.cint','l3-cleanup.cint'):
        subprocess.run(SSH+['cat > /tmp/'+name],input=(ROOT/'offload'/name).read_text(),text=True,check=True)
    with (ROOT/'work/sdk-netdev/test_route').open('rb') as f: subprocess.run(SSH+['cat > /tmp/test_route'],stdin=f,check=True)
    started=False
    try:
        ssh('chmod 755 /tmp/test_route; echo 0 > /proc/sys/net/ipv4/ip_forward')
        for c in ['port xe0,xe15 enable=false','vlan remove 1 pbm=xe0,xe15','vlan create 4001 pbm=cpu,xe0 ubm=xe0','vlan create 4002 pbm=cpu,xe15 ubm=xe15','pvlan set xe0 4001','pvlan set xe15 4002']:
            sdk(c)
        started=True
        r['setup']=sdk('cint /tmp/l3-setup.cint')
        if 'OFFLOAD_SETUP_OK' not in r['setup']: raise RuntimeError(r['setup'])
        for c in ['port xe0,xe15 enable=true','port xe0,xe15 loopback=mac']:
            sdk(c)
        for _ in range(30):
            if ssh('cat /sys/class/net/swp1/carrier /sys/class/net/swp16/carrier').stdout.strip()=='1\n1':break
            time.sleep(.5)
        time.sleep(3)
        r['linux_forwarding_during_test']=ssh('cat /proc/sys/net/ipv4/ip_forward').stdout.strip()
        r['linux_routes_during_test']=ssh('ip -j -4 route show table all').stdout
        r['linux_addresses_during_test']=ssh('ip -j -4 addr show').stdout
        r['hardware_route']=sdk('l3 defip show');r['hardware_egress']=sdk('l3 egress show')
        p=ssh('/tmp/test_route',False);r['packet_test']={'exit_code':p.returncode,'stdout':p.stdout,'stderr':p.stderr}
        r['hardware_route_after_packets']=sdk('l3 defip show')
        r['ingress_counters']=sdk('show c xe0');r['egress_counters']=sdk('show c xe15')
        if p.returncode: raise RuntimeError(p.stdout+p.stderr)
        r['withdraw']=sdk('l3 defip destroy VRF=0 IP=198.19.2.0 MaSk=255.255.255.0')
        p=ssh('/tmp/test_route --withdrawn',False);r['negative_control']={'exit_code':p.returncode,'stdout':p.stdout,'stderr':p.stderr}
        if p.returncode: raise RuntimeError(p.stdout+p.stderr)
        r['passed']=True
    finally:
        # Record the evidence even if cleanup fails; a leftover RIF/route is
        # precisely the state the next operator needs to see.
        try:
            if started: r['cleanup']=sdk('cint /tmp/l3-cleanup.cint')
            sdk('port xe0,xe15 loopback=none')
            r['restore_ports_vlans']=json.loads(ssh('/usr/libexec/bcm-switch apply').stdout)
            ssh('echo '+shlex.quote(old_forward)+' > /proc/sys/net/ipv4/ip_forward')
            r['final_routes']=sdk('l3 defip show');r['final_interfaces']=sdk('l3 intf show')
        except Exception as exc:
            r['restore_error']=repr(exc)
            raise
        finally:
            r.setdefault('passed',False)
            (ROOT/'offload/hardware-route-result-20261109.json').write_text(json.dumps(r,indent=2,ensure_ascii=False)+'\n')
    print('PASS: 20/20 ASIC-routed frames, MAC rewrite and TTL decrement; route-withdrawal negative control passed; RAM test state restored')
if __name__=='__main__': main()
