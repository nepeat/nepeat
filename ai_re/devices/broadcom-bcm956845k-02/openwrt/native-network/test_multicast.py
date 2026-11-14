#!/usr/bin/env python3
"""Additional native tagged-multicast hardware proof; restore all test intent."""
import datetime,json,subprocess
import test_hardware as h

def main():
    backup=h.ssh('cat /etc/config/network').stdout
    result={'date_utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),
            'deployment':'same RAM image, after restoring original helper from unsupported PFM API probe',
            'manifest':json.loads((h.ROOT/'artifacts/manifest.json').read_text())}
    capture=False
    try:
        h.wait_for(lambda st,p:st['rif_count']==0,'baseline healthy after PFM probe')
        with (h.ROOT/'work/sdk-netdev/test_native_bridge').open('rb') as f:
            subprocess.run(h.SSH+['cat > /tmp/test_native_bridge'],stdin=f,check=True)
        h.ssh('chmod 755 /tmp/test_native_bridge')
        for name in ('capture-setup.cint','capture-cleanup.cint'):
            subprocess.run(h.SSH+['cat > /tmp/'+name],input=(h.ROOT/'native-network'/name).read_text(),text=True,check=True)
        h.batch("""del_list network.switch_vlan1.ports='swp1:u*'
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
        result['status'],_=h.wait_for(lambda st,p:p['ports'][0]['pvid']==100 and p['ports'][15]['pvid']==200,'native multicast VLAN')
        out=h.sdk('cint /tmp/capture-setup.cint');assert 'NATIVE_CAPTURE_OK' in out;capture=True
        h.ready_loopback()
        result['vlans']=h.sdk('vlan show')
        p=h.ssh('/tmp/test_native_bridge --tagged --multicast',False)
        result['packets']={'rc':p.returncode,'stdout':p.stdout,'stderr':p.stderr};print(p.stdout,flush=True)
        if p.returncode:raise RuntimeError(p.stdout+p.stderr)
        result['passed']=True
    except BaseException as error:
        result['error']=str(error);raise
    finally:
        errors=[]
        def clean(label,action):
            try:action()
            except Exception as error:errors.append(label+': '+str(error))
        clean('loopbacks',lambda:h.sdk('port xe0,xe15 loopback=none'))
        if capture:clean('capture',lambda:h.sdk('cint /tmp/capture-cleanup.cint'))
        clean('network restore',lambda:subprocess.run(h.SSH+['cat > /etc/config/network'],input=backup,text=True,check=True))
        clean('network reload',lambda:h.ssh('ubus call network reload'))
        def converged():
            result['final_status'],_=h.wait_for(lambda st,p:len(p['topology']['vlans'])==1 and p['topology']['vlans'][0]['vid']==1,'default bridge restored')
        clean('convergence',converged)
        if errors:result['cleanup_errors']=errors;result['passed']=False
        (h.ROOT/'native-network/multicast-result-20261109.json').write_text(json.dumps(result,indent=2)+'\n')
        if errors:raise RuntimeError(str(errors))
if __name__=='__main__':main()
