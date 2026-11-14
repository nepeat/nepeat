#!/usr/bin/env python3
"""Native config -> SDK -> real packet tests, RAM-only ports 1/16.

Requires the default br-switch configuration and active native controller.
Hardware SDK commands are used only for loopback/capture instrumentation and
readback. Bridge, VLAN, IP, FDB and route changes use native Linux/netifd.
"""
from pathlib import Path
import datetime,json,subprocess,time,shlex,os
ROOT=Path(__file__).resolve().parent.parent
SSH=['bash',str(ROOT/'ssh.sh')]
def ssh(c,check=True):
    p=subprocess.run(SSH+[c],capture_output=True,text=True,timeout=90)
    if check and p.returncode:raise RuntimeError(p.stdout+p.stderr)
    return p

def sdk(c):
    j=json.loads(ssh('bcmctl --json '+shlex.quote(c)).stdout)
    if not j['ok']:raise RuntimeError(j)
    return j['output']

def current():
    current=json.loads(ssh('cat /tmp/bcm-sdk/native-current.json').stdout)
    return current['status'],current['plan']

def wait_for(predicate,label):
    last=None
    for _ in range(60):
        try:
            st,p=current();last=st
            if st.get('ok') and predicate(st,p):
                objects=st.get('objects')
                if objects:
                    assert objects['rifs']==len(p['rifs']),('RIF leak',objects)
                    assert objects['routes']==len(p['routes']),('route leak',objects)
                    assert objects['egress']==len(p['nhops'])+(1 if p['routes'] else 0),('next-hop leak',objects)
                return st,p
        except (ValueError,RuntimeError):pass
        time.sleep(.5)
    raise RuntimeError('convergence failed: '+label+' '+str(last))

def batch(text):
    subprocess.run(SSH+['uci batch'],input=text,text=True,check=True)
    ssh('ubus call network reload')

def ready_loopback():
    sdk('port xe0,xe15 loopback=mac')
    # The legacy diagnostic loopback setter silently changes speed to 10G.
    # Restore the configured 40G profile before measuring packet behavior.
    sdk('port xe0,xe15 speed=40000')
    wait_for(lambda st,p:all(p['ports'][n]['state']==4 for n in (0,15)),'bridge ports forwarding')
    time.sleep(3)

def main():
    backup=ssh('cat /etc/config/network').stdout
    result={'date_utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),
            'deployment':'fresh RAM image boot' if os.getenv('BCM_NATIVE_IMAGE_BOOT')=='1' else 'RAM hot-deployment prototype',
            'manifest':json.loads((ROOT/'artifacts/manifest.json').read_text())}
    capture=False
    result_stem=os.getenv('BCM_TEST_RESULT','hardware-result')
    try:
        result['watcher_before']=json.loads(ssh('cat /tmp/bcm-sdk/watcher-status.json').stdout)
        result['initial_status'],_=wait_for(lambda st,p:True,'native controller ready')
        assert result['initial_status']['ok'] and ssh('cat /sys/class/net/swp1/phys_switch_id').stdout.strip()=='62636d3536383435'
        for name in ('test_native_bridge','test_native_route','test_native_arp'):
            with (ROOT/'work/sdk-netdev'/name).open('rb') as f:subprocess.run(SSH+['cat > /tmp/'+name],stdin=f,check=True)
        ssh('chmod 755 /tmp/test_native_bridge /tmp/test_native_route /tmp/test_native_arp')
        for name in ('capture-setup.cint','capture-cleanup.cint'):
            subprocess.run(SSH+['cat > /tmp/'+name],input=(ROOT/'native-network'/name).read_text(),text=True,check=True)
        if os.getenv('BCM_L3_ONLY')!='1':
            batch("""del_list network.switch_vlan1.ports='swp1:u*'
    del_list network.switch_vlan1.ports='swp16:u*'
    set network.native_test100=bridge-vlan
    set network.native_test100.device='br-switch'
    set network.native_test100.vlan='100'
    add_list network.native_test100.ports='swp1:u*'
    add_list network.native_test100.ports='swp16:u*'
    commit network
    """)
            result['untagged_status'],_=wait_for(lambda st,p:p['ports'][0]['pvid']==100 and p['ports'][15]['pvid']==100,'native VLAN 100')
            result['capture_setup']=sdk('cint /tmp/capture-setup.cint');assert 'NATIVE_CAPTURE_OK' in result['capture_setup'];capture=True
            ready_loopback()
            result['untagged_vlans']=sdk('vlan show')
            p=ssh('/tmp/test_native_bridge',False);result['untagged_packets']={'rc':p.returncode,'stdout':p.stdout,'stderr':p.stderr};print(p.stdout,flush=True)
            if p.returncode:raise RuntimeError(p.stdout+p.stderr)
            batch("""del_list network.native_test100.ports='swp16:u*'
    add_list network.native_test100.ports='swp16:t'
    set network.native_test200=bridge-vlan
    set network.native_test200.device='br-switch'
    set network.native_test200.vlan='200'
    add_list network.native_test200.ports='swp16:u*'
    commit network
    """)
            result['tagged_status'],_=wait_for(lambda st,p:p['ports'][15]['pvid']==200 and any(v['vid']==100 and 16 in v['members'] and 16 not in v['untagged'] for v in p['topology']['vlans']),'tagged egress VLAN 100')
            ready_loopback()
            result['tagged_vlans']=sdk('vlan show')
            p=ssh('/tmp/test_native_bridge --tagged',False);result['tagged_packets']={'rc':p.returncode,'stdout':p.stdout,'stderr':p.stderr};print(p.stdout,flush=True)
            if p.returncode:raise RuntimeError(p.stdout+p.stderr)
            p=ssh('/tmp/test_native_bridge --tagged --multicast',False);result['multicast_packets']={'rc':p.returncode,'stdout':p.stdout,'stderr':p.stderr};print(p.stdout,flush=True)
            if p.returncode:raise RuntimeError(p.stdout+p.stderr)
        else:
            batch("""del_list network.switch_vlan1.ports='swp1:u*'
del_list network.switch_vlan1.ports='swp16:u*'
set network.native_test100=bridge-vlan
set network.native_test100.device='br-switch'
set network.native_test100.vlan='100'
add_list network.native_test100.ports='swp1:u*'
add_list network.native_test100.ports='swp16:t'
set network.native_test200=bridge-vlan
set network.native_test200.device='br-switch'
set network.native_test200.vlan='200'
add_list network.native_test200.ports='swp16:u*'
commit network
""")
            wait_for(lambda st,p:p['ports'][0]['pvid']==100 and p['ports'][15]['pvid']==200,'L3 test VLANs')
            result['capture_setup']=sdk('cint /tmp/capture-setup.cint');assert 'NATIVE_CAPTURE_OK' in result['capture_setup'];capture=True
        # Move swp16 exclusively to VLAN 200 for the routed test.
        batch("""del_list network.native_test100.ports='swp16:t'
set network.native_in=interface
set network.native_in.proto='static'
set network.native_in.device='br-switch.100'
set network.native_in.ipaddr='198.18.1.1'
set network.native_in.netmask='255.255.255.0'
set network.native_out=interface
set network.native_out.proto='static'
set network.native_out.device='br-switch.200'
set network.native_out.ipaddr='198.18.2.1'
set network.native_out.netmask='255.255.255.0'
set network.native_route=route
set network.native_route.interface='native_out'
set network.native_route.target='198.19.2.0'
set network.native_route.netmask='255.255.255.0'
set network.native_route.gateway='198.18.2.2'
commit network
""")
        result['rif_status'],_=wait_for(lambda st,p:st['rif_count']==2,'native IPv4 router interfaces')
        ready_loopback()
        p=ssh('/tmp/test_native_arp swp1',False);result['local_arp']={'rc':p.returncode,'stdout':p.stdout,'stderr':p.stderr};print(p.stdout,flush=True)
        if p.returncode:raise RuntimeError(p.stdout+p.stderr)
        ssh('bridge fdb replace 02:10:18:bb:02:02 dev swp16 master static vlan 200; ip neigh replace 198.18.2.2 lladdr 02:10:18:bb:02:02 dev br-switch.200 nud permanent')
        result['route_status'],_=wait_for(lambda st,p:any(r['ip']==0xc6130200 and r['bits']==24 and r['kind']=='hardware' for r in p['routes']),'resolved route programmed in ASIC')
        ready_loopback()
        result['linux_routes']=json.loads(ssh('ip -j -4 route show table main').stdout)
        result['linux_neighbors']=json.loads(ssh('ip -j -4 neigh show').stdout)
        result['hardware_route_before']=sdk('l3 defip show')
        p=ssh('/tmp/test_native_route',False);result['routed_packets']={'rc':p.returncode,'stdout':p.stdout,'stderr':p.stderr};print(p.stdout,flush=True)
        result['hardware_route_after']=sdk('l3 defip show')
        if p.returncode:raise RuntimeError(p.stdout+p.stderr)
        # Same-priority policy selector must stop hardware forwarding, without
        # deleting/recreating any RIF. Linux supplies the policy semantics.
        stable=result['route_status']['objects']
        started=time.monotonic()
        ssh('ip rule add priority 32766 fwmark 0x40 lookup main')
        result['policy_status'],_=wait_for(lambda st,p:st['hardware_routes']==0 and p['warnings'],'same-priority policy rule to CPU')
        result['policy_convergence_seconds']=time.monotonic()-started
        assert result['policy_status']['objects']['rif_creates']==stable['rif_creates']
        assert result['policy_status']['objects']['rif_deletes']==stable['rif_deletes']
        p=ssh('/tmp/test_native_route --cpu',False);result['policy_cpu_forwarding']={'rc':p.returncode,'stdout':p.stdout,'stderr':p.stderr};print(p.stdout,flush=True)
        if p.returncode:raise RuntimeError(p.stdout+p.stderr)
        ssh('ip rule del priority 32766 fwmark 0x40 lookup main')
        result['policy_recovery'],_=wait_for(lambda st,p:any(r['ip']==0xc6130200 and r['bits']==24 and r['kind']=='hardware' for r in p['routes']),'policy removal restores hardware route')
        stable=result['policy_recovery']['objects']
        # Keep the resolved route but withdraw its neighbor; must punt, not use
        # a stale hardware next-hop. Kernel ARP can resolve it again normally.
        ssh('ip neigh del 198.18.2.2 dev br-switch.200')
        result['withdraw_status'],_=wait_for(lambda st,p:any(r['ip']==0xc6130200 and r['bits']==24 and r['kind']=='cpu' for r in p['routes']),'neighbor withdrawal to CPU')
        assert result['withdraw_status']['objects']['rif_creates']==stable['rif_creates']
        assert result['withdraw_status']['objects']['rif_deletes']==stable['rif_deletes']
        assert result['withdraw_status']['objects']['egress_creates']==stable['egress_creates']
        result['incremental_preserved_rifs_and_cpu_egress']=True
        p=ssh('/tmp/test_native_route --withdrawn',False);result['neighbor_negative']={'rc':p.returncode,'stdout':p.stdout,'stderr':p.stderr};print(p.stdout,flush=True)
        if p.returncode:raise RuntimeError(p.stdout+p.stderr)
        # A native blackhole must supersede the previously resolved SDK route.
        ssh('ip neigh replace 198.18.2.2 lladdr 02:10:18:bb:02:02 dev br-switch.200 nud permanent; ip route replace blackhole 198.19.2.0/24')
        result['blackhole_status'],_=wait_for(lambda st,p:any(r['ip']==0xc6130200 and r['bits']==24 and r['kind']=='cpu' for r in p['routes']) and any(n['port']==16 for n in p['nhops']),'native blackhole with resolved neighbor')
        p=ssh('/tmp/test_native_route --withdrawn',False);result['blackhole_negative']={'rc':p.returncode,'stdout':p.stdout,'stderr':p.stderr};print(p.stdout,flush=True)
        if p.returncode:raise RuntimeError(p.stdout+p.stderr)
        ssh('ip route del blackhole 198.19.2.0/24')
        result['watcher_after']=json.loads(ssh('cat /tmp/bcm-sdk/watcher-status.json').stdout)
        assert result['watcher_after']['netlink_reconciles']>result['watcher_before']['netlink_reconciles']
        result['passed']=True
    except BaseException as error:
        result['error']=str(error)
        for label,command in [('plan','cat /tmp/bcm-sdk/native-plan.json'),('netdevs','cat /tmp/bcm-sdk/netdev-status.json'),('links','ip -j -d link show')]:
            try:result['failure_'+label]=ssh(command).stdout
            except Exception:pass
        for command in ['ps','port xe0,xe15','stg show','show c']:
            try:result['failure_sdk_'+command]=sdk(command)
            except Exception:pass
        raise
    finally:
        # Attempt each restoration even if a preceding SDK action fails.
        cleanup=[]
        def restore(label, action):
            try:action()
            except Exception as error:cleanup.append(label+': '+str(error))
        restore('policy rule',lambda:ssh('ip rule del priority 32766 fwmark 0x40 lookup main',False))
        restore('blackhole',lambda:ssh('ip route del blackhole 198.19.2.0/24',False))
        restore('loopbacks',lambda:sdk('port xe0,xe15 loopback=none'))
        if capture:restore('capture',lambda:result.update(capture_cleanup=sdk('cint /tmp/capture-cleanup.cint')))
        restore('FDB',lambda:ssh('bridge fdb del 02:10:18:bb:02:02 dev swp16 master vlan 200',False))
        restore('network restore',lambda:subprocess.run(SSH+['cat > /etc/config/network'],input=backup,text=True,check=True))
        restore('network reload',lambda:ssh('ubus call network reload'))
        def converge():
            result['final_status'],_=wait_for(lambda st,p:st['rif_count']==0 and len(p['topology']['vlans'])==1 and p['topology']['vlans'][0]['vid']==1,'restore default native bridge')
        restore('default convergence',converge)
        if cleanup:result['cleanup_errors']=cleanup;result['passed']=False
        stamp=datetime.datetime.now(datetime.timezone.utc).strftime('%Y%m%dT%H%M%SZ')
        data=json.dumps(result,indent=2,ensure_ascii=False)+'\n'
        (ROOT/'native-network'/(result_stem+'-'+stamp+'.json')).write_text(data)
        (ROOT/'native-network'/(result_stem+'-20261109.json')).write_text(data)
        if cleanup:raise RuntimeError('cleanup failed: '+str(cleanup))
    print('PASS: native VLAN tagging, no duplicate forwarding, IPv4 hardware routing and neighbor-withdrawal fallback; default config restored')
if __name__=='__main__':main()
