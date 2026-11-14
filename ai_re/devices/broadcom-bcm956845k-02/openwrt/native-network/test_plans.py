#!/usr/bin/env python3
"""Semantic tests for the native planner; no hardware writes (fixture mode)."""
from pathlib import Path
import ipaddress,json,subprocess
ROOT=Path(__file__).resolve().parent.parent
SSH=['bash',str(ROOT/'ssh.sh')]
def base():
    links=[{'ifindex':p,'ifname':f'swp{p}','address':f'02:10:18:56:00:{p:02x}','mtu':1500,'flags':['UP','LOWER_UP'],'linkinfo':{'info_kind':'tun'}} for p in range(1,17)]
    return dict(links=links,vlans=[],fdb=[],addresses=[],routes=[],neighbors=[],rules=[{'priority':p,'src':'all','table':t} for p,t in [(0,'local'),(32766,'main'),(32767,'default')]],forwarding=True,hardware={'core':{'profile':'40g'}},network={},firewall={},hardware_fdb='')
def routed():
    s=base();s['links'].append({'ifindex':100,'ifname':'br-switch','address':'02:10:18:56:ff:00','mtu':1500,'linkinfo':{'info_kind':'bridge','info_data':{'vlan_filtering':1,'mcast_snooping':0,'stp_state':0}}})
    for p,vid in [(1,100),(16,200)]:
        s['links'][p-1]['master']='br-switch';s['links'][p-1]['linkinfo']['info_slave_data']={'state':'forwarding','learning':True}
        s['vlans'].append({'ifname':f'swp{p}','vlans':[{'vlan':vid,'flags':['PVID','Egress Untagged']}]})
        name=f'br-switch.{vid}'
        s['links'].append({'ifindex':100+vid,'ifname':name,'link_index':100,'address':'02:10:18:56:ff:00','mtu':1500,'linkinfo':{'info_kind':'vlan','info_data':{'id':vid}}})
        s['addresses'].append({'ifname':name,'addr_info':[{'family':'inet','local':f'198.18.{1 if vid==100 else 2}.1','prefixlen':24}]})
        s['routes'].append({'dst':f'198.18.{1 if vid==100 else 2}.0/24','dev':name,'scope':'link'})
    s['vlans'].append({'ifname':'br-switch','vlans':[{'vlan':100},{'vlan':200}]})
    s['routes'] += [{'dst':'198.19.2.0/24','dev':'br-switch.200','gateway':'198.18.2.2'},{'dst':'default','dev':'eth0','gateway':'10.60.2.1'}]
    s['neighbors']=[{'dst':'198.18.2.2','dev':'br-switch.200','lladdr':'02:10:18:bb:02:02','state':['PERMANENT']}]
    s['fdb']=[{'mac':'02:10:18:bb:02:02','ifname':'swp16','vlan':200,'master':'br-switch','state':'static','flags':[]}]
    return s

def main():
    results=[]
    def run(name,s,check=None,error=None):
        subprocess.run(SSH+['cat > /tmp/native-fixture.json'],input=json.dumps(s),text=True,check=True)
        p=subprocess.run(SSH+['ucode /usr/libexec/bcm-native.uc fixture /tmp/native-fixture.json'],text=True,capture_output=True,timeout=20)
        if error:
            assert p.returncode and error in p.stderr,(name,p.stdout,p.stderr)
        else:
            assert p.returncode==0,(name,p.stdout,p.stderr)
            obj=json.loads(p.stdout);check(obj)
        results.append({'case':name,'passed':True})
    def expect(condition): assert condition
    run('standalone ports are isolated',base(),lambda p:expect(len({x['pvid'] for x in p['ports']})==16 and all(len(v['members'])==1 for v in p['topology']['vlans'])))
    def route(p):return next(r for r in p['routes'] if r['ip']==int(ipaddress.IPv4Address('198.19.2.0')) and r['bits']==24)
    run('resolved route offloads through static FDB port',routed(),lambda p:expect(route(p)['kind']=='hardware' and p['nhops'][0]['port']==16))
    s=routed();s['neighbors']=[];run('neighbor withdrawal falls back to CPU',s,lambda p:expect(route(p)['kind']=='cpu' and not p['nhops']))
    s=routed();s['forwarding']=False;run('Linux forwarding disabled never offloads transit',s,lambda p:expect(all(r['kind']!='hardware' for r in p['routes'])))
    s=routed();s['rules'].append({'priority':100});run('policy routing stays in CPU',s,lambda p:expect(all(r['kind']!='hardware' for r in p['routes']) and p['warnings']))
    s=routed();s['firewall']={'rule':{'.type':'rule'}};run('configured firewall keeps routing in CPU',s,lambda p:expect(all(r['kind']!='hardware' for r in p['routes'])))
    s=routed();s['routes'].append({'dst':'198.19.2.0/24','type':'blackhole'});run('blackhole is not bypassed by a hardware prefix',s,lambda p:expect(route(p)['kind']=='cpu'))
    s=routed();s['routes'][-2]['nexthops']=[{'gateway':'198.18.2.2'}];run('unsupported ECMP falls back to CPU',s,lambda p:expect(route(p)['kind']=='cpu'))
    s=routed();s['rules'][1]['table']=100;run('standard priority with alternate table stays in CPU',s,lambda p:expect(all(r['kind']!='hardware' for r in p['routes'])))
    s=routed();s['rules'][1]['fwmark']='0x1';run('standard priority with selector stays in CPU',s,lambda p:expect(all(r['kind']!='hardware' for r in p['routes'])))
    s=routed();s['rules'].pop();run('incomplete default rule chain stays in CPU',s,lambda p:expect(all(r['kind']!='hardware' for r in p['routes'])))
    s=routed();s['rules'][2]=s['rules'][1].copy();run('duplicate standard rule stays in CPU',s,lambda p:expect(all(r['kind']!='hardware' for r in p['routes'])))
    s=routed();s['routes'][-2]['tos']='0x10';run('TOS-selected routes disable destination-only offload',s,lambda p:expect(all(r['kind']!='hardware' for r in p['routes'])))
    s=routed();s['routes'][-2]['metrics']={'mtu':1200};run('per-route MTU uses CPU',s,lambda p:expect(route(p)['kind']=='cpu'))
    s=routed();s['routes'][-2]['flags']=['linkdown'];run('link-down route uses CPU',s,lambda p:expect(route(p)['kind']=='cpu'))
    s=routed();s['links'][15]['flags']=[];run('disabled egress port cannot resolve hardware next hop',s,lambda p:expect(route(p)['kind']=='cpu'))
    s=routed();s['links'][0]['mtu']=9000;run('unsupported MTU rejected',s,error='MTU 1500')
    s=routed();next(l for l in s['links'] if l['ifname']=='br-switch')['linkinfo']['info_data']['mcast_snooping']=1
    run('multicast snooping is not silently ignored',s,error='multicast snooping')
    s=routed();next(l for l in s['links'] if l['ifname']=='br-switch')['linkinfo']['info_data']['stp_state']=1;run('unvalidated STP rejected',s,error='STP protocol')
    s=routed();s['vlans'][0]['vlans'][0]['vlan']=4090;run('reserved private VLAN cannot leak into user bridge',s,error='reserved')
    s=routed();s['links'][0]['linkinfo']['info_slave_data']['isolated']=True;run('unsupported isolation policy rejected',s,error='unsupported bridge-port policy')
    s=routed();s['links'].append({'ifindex':900,'ifname':'eth0','master':'br-switch','linkinfo':{},'address':'02:00:00:00:00:01','mtu':1500});run('management/software bridge mixing rejected',s,error='mixing CPU/software')
    out=ROOT/'native-network/planner-tests-20261109.json';out.write_text(json.dumps({'passed':len(results),'tests':results},indent=2)+'\n')
    print(f'PASS: {len(results)} semantic planner cases; fixture mode performed no hardware writes')
if __name__=='__main__':main()
