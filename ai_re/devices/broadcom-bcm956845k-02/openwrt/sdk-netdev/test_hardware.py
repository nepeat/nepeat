#!/usr/bin/env python3
"""Explicit RAM-only SDK/TAP hardware test; interrupts ports 1 and 16.

Run inside the RE nix shell after booting the matching image. Requires the
compiled test_packet/test_arp under work/sdk-netdev, and a Chromium CDP test
browser on loopback 9223. Restores network config and disables MAC loopback.
"""
from pathlib import Path
import datetime,json,subprocess,time,os
ROOT=Path(__file__).resolve().parent.parent
SSH=['bash',str(ROOT/'ssh.sh')]
def ssh(command,check=True):
    p=subprocess.run(SSH+[command],text=True,capture_output=True,timeout=90)
    if check and p.returncode: raise RuntimeError(p.stdout+p.stderr)
    return p.stdout

def sdk(command):
    import shlex
    result=json.loads(ssh('bcmctl --json '+shlex.quote(command)))
    if not result['ok']: raise RuntimeError(result)
    return result['output']

def wait_for(check,message):
    for _ in range(30):
        if check(): return
        time.sleep(.5)
    raise RuntimeError(message)

def main():
    result={'tested_at_utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),
            'manifest':json.loads((ROOT/'artifacts/manifest.json').read_text()),
            'backend':'SDK packet API/TAP; not Broadcom KNET'}
    backup=ssh('cat /etc/config/network')
    try:
        result['initial_status']=json.loads(ssh('/usr/libexec/bcm-switch status'))
        assert result['initial_status']['initialized'] and result['initial_status']['last_apply_ok']
        result['initial_devices']=json.loads(ssh('ip -j -d link show'))
        ports=[p for p in result['initial_devices'] if p['ifname'].startswith('swp')]
        assert len(ports)==16
        result['initial_speed']=ssh('cat /sys/class/net/swp1/speed').strip()
        assert result['initial_speed']=='40000'
        for name in ('test_packet','test_arp'):
            with (ROOT/'work/sdk-netdev'/name).open('rb') as f:
                subprocess.run(SSH+['cat > /tmp/'+name],stdin=f,check=True,timeout=30)
        ssh('chmod 755 /tmp/test_packet /tmp/test_arp')
        ssh('ip link set dev swp1 down')
        wait_for(lambda:'DISABLED' in sdk('port xe0'),'Linux down did not disable ASIC')
        result['linux_admin_down']=sdk('port xe0')
        ssh('ip link set dev swp1 up')
        wait_for(lambda:'DISABLED' not in sdk('port xe0'),'Linux up did not enable ASIC')
        result['linux_admin_up']=sdk('port xe0')
        result['packet_tests']={}
        for port,asic in [('swp1','xe0'),('swp16','xe15')]:
            sdk('port '+asic+' loopback=mac')
            wait_for(lambda:ssh('cat /sys/class/net/'+port+'/carrier').strip()=='1','Carrier did not rise for '+port)
            time.sleep(2) # Allow vendor linkscan/MAC settings to settle.
            result['packet_tests'][port]=ssh('/tmp/test_packet '+port)
            assert '20/20' in result['packet_tests'][port]
            if port!='swp1': sdk('port '+asic+' loopback=none')
        ssh('uci set network.sdk_test=interface; uci set network.sdk_test.proto=static; '
            'uci set network.sdk_test.device=swp1; uci set network.sdk_test.ipaddr=198.18.0.1; '
            'uci set network.sdk_test.netmask=255.255.255.252; uci commit network; '
            'ubus call network reload; ifup sdk_test')
        wait_for(lambda:json.loads(ssh('ubus call network.interface.sdk_test status')).get('up'), 'netifd IP did not come up')
        result['netifd']=json.loads(ssh('ubus call network.interface.sdk_test status'))
        result['arp']=ssh('/tmp/test_arp swp1');assert 'PASS' in result['arp']
        env=dict(os.environ,BCM_LUCI_NATIVE='1')
        p=subprocess.run(['node',str(ROOT/'test_luci.mjs')],env=env,capture_output=True,text=True,timeout=90)
        result['luci']=json.loads((ROOT/'work/luci-native-browser-result.json').read_text())
        if p.returncode: raise RuntimeError('Native LuCI check failed: '+p.stderr)
        assert result['luci']['hasNativePort'] and not result['luci']['exceptions']
        result['netdev_status']=json.loads(ssh('cat /tmp/bcm-sdk/netdev-status.json'))
        result['passed']=True
    finally:
        # Restore the device, but record the evidence even when restoration fails:
        # a half-restored switch is exactly when the partial record matters most.
        try:
            subprocess.run(SSH+['cat > /etc/config/network'],input=backup,text=True,check=True)
            ssh('ubus call network reload')
            for asic in ('xe0','xe15'): sdk('port '+asic+' loopback=none')
            result['restore']=json.loads(ssh('/usr/libexec/bcm-switch apply'))
            time.sleep(2)
            result['final_status']=json.loads(ssh('/usr/libexec/bcm-switch status'))
            result['final_devices']=json.loads(ssh('ip -j link show'))
            result['final_speed']=ssh('cat /sys/class/net/swp1/speed').strip()
        except Exception as exc:
            result['restore_error']=repr(exc)
            raise
        finally:
            result.setdefault('passed',False)
            (ROOT/'sdk-netdev/hardware-result-20261109.json').write_text(json.dumps(result,indent=2,ensure_ascii=False)+'\n')
    print('PASS: 16 Linux ports, admin down/up, 20/20 frames on ports 1 and 16, netifd IPv4/ARP, native LuCI; defaults restored')
if __name__=='__main__': main()
