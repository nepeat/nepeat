#!/usr/bin/ucode
/* SPDX-License-Identifier: GPL-2.0-only
 * Linux is authoritative. SDK state is reconciled, never inferred from success
 * of a Linux command alone. One VLAN-aware ASIC-only bridge, IPv4 main table.
 */
import * as fs from 'fs';
import * as uci from 'uci';
const ROOT='/tmp/bcm-sdk';
const LOCK='/var/lock/bcm-switch-apply';
let transaction=0;
function read(p, fallback) { let s=fs.readfile(p); return s == null ? fallback : trim(s); }
function load(p, fallback) { try { return json(fs.readfile(p)); } catch(e) { return fallback; } }
function write(p,v) { if(!fs.writefile(p+'.new',sprintf('%J',v)+'\n')||!fs.rename(p+'.new',p))die('cannot publish '+p); }
function run(c) {
    let p=fs.popen(c+' 2>/dev/null','r'); if(!p) die('cannot run '+c);
    let out=p.read('all');let rc=p.close();if(rc) die('command failed: '+c);
    return out;
}
function shell(s) { return "'"+replace(s,/'/g,"'\\''")+"'"; }
function sdk(c) { let r=json(run('bcmctl --json '+shell(c))); if(!r.ok) die(r.output || 'SDK error');return r.output; }
function objarray(o) { let a=[];for(let k,v in o || {})push(a,v);return a; }
function has(a,v) {return index(a || [],v)>=0;}
function portid(n) {let m=match(n || '',/^swp([1-9][0-9]*)$/);return m?int(m[1]):0;}
function mac(m) {if(!match(m || '',/^[0-9a-fA-F]{2}(:[0-9a-fA-F]{2}){5}$/))die('invalid MAC address');return lc(m);}
function ipnum(s) {
    let p=split(s || '', '.');if(length(p)!=4)die('invalid IPv4 address');let n=0;
    for(let v in p) {if(!match(v,/^[0-9]+$/)||int(v)>255)die('invalid IPv4 octet');n=n*256+int(v);}
    return n;
}
function prefix(s) {let a=split(s || '0.0.0.0/0','/');if(a[0]=='default')a=['0.0.0.0','0'];let bits=length(a)>1?int(a[1]):32;if(bits<0||bits>32)die('invalid prefix');let mask=bits?((0xffffffff << (32-bits)) & 0xffffffff):0;return {ip:ipnum(a[0])&mask,mask,bits};}
function snapshot() {
    let c=uci.cursor();
    return {generation:read(ROOT+'/native-generation','legacy'),links:json(run('ip -j -d link show')),vlans:json(run('bridge -j vlan show')),
        fdb:json(run('bridge -j fdb show')),addresses:json(run('ip -j -4 addr show')),
        routes:json(run('ip -j -4 route show table main')),neighbors:json(run('ip -j -4 neigh show')),
        rules:json(run('ip -j -4 rule show')),forwarding:read('/proc/sys/net/ipv4/ip_forward','0')=='1',
        hardware:c.get_all('bcm_switch') || {},network:c.get_all('network') || {},firewall:c.get_all('firewall') || {},
        hardware_fdb:sdk('l2 show')};
}
function plan(s) {
    let profile=s.hardware.core?.profile || '40g';if(profile!='40g'&&profile!='10g')die('invalid lane profile');
    let count=profile=='40g'?16:64;
    let links={},indices={};for(let l in s.links) {links[l.ifname]=l;indices[l.ifindex]=l.ifname;}
    let ports=[],bridge=null,bridgeNames={};
    for(let p=1;p<=count;p++) {
        let name='swp'+p,l=links[name];if(!l)die('waiting for Linux device '+name);
        if(l.mtu!=1500)die('this first hardware integration requires MTU 1500');
        let master=type(l.master)=='integer'?indices[l.master]:l.master;
        if(master) {
            let b=links[master];if(b?.linkinfo?.info_kind!='bridge')die(name+' has an unsupported master');
            bridgeNames[master]=true;
        }
        let h=s.hardware['xe'+(p-1)] || {};let speed=int(h.speed || '0');if(!speed)speed=profile=='40g'?40000:10000;
        if(!has([1000,10000,40000],speed)||(profile=='10g'&&speed==40000))die('unsupported hardware speed');
        let bp=l.linkinfo?.info_slave_data || {};
        if(bp.hairpin||bp.isolated||bp.locked||bp.mcast_to_unicast||bp.neigh_suppress||bp.vlan_tunnel||bp.proxy_arp||bp.proxy_arp_wifi)die(name+' has an unsupported bridge-port policy');
        if(bp.flood===false||bp.mcast_flood===false||bp.bcast_flood===false)die('per-port flood suppression is not supported yet');
        let state=bp.state || 'forwarding';
        let stp={disabled:0,listening:2,learning:3,forwarding:4,blocking:1};
        push(ports,{name,id:p,master:master || null,mac:mac(l.address),pvid:4030+p,admin:has(l.flags,'UP'),
            state:master?(stp[state] ?? 0):4,learning:bp.learning!==false,speed,autoneg:int(h.autoneg || '0')});
    }
    let bn=keys(bridgeNames);if(length(bn)>1)die('only one hardware bridge is supported');
    if(length(bn)) {
        let l=links[bn[0]],d=l.linkinfo.info_data;
        if(!d.vlan_filtering)die('enable VLAN filtering on '+l.ifname);
        if(d.stp_state)die('kernel STP protocol offload is not validated yet');
        if(l.mtu!=1500)die('the hardware bridge requires MTU 1500');
        if(d.mcast_snooping)die('disable multicast snooping for the supported hardware bridge mode');
        bridge={name:l.ifname,mac:mac(l.address)};
        for(let l in s.links) {
            let m=type(l.master)=='integer'?indices[l.master]:l.master;
            if(m==bridge.name&&!portid(l.ifname))die('mixing CPU/software ports with switch ports is unsupported');
        }
    }
    for(let section in objarray(s.network)) if(section['.type']=='device' && section.type=='bridge')
        for(let name in section.ports || []) if(portid(name)>count)die(name+' is inactive in this lane profile');
    let vs={},self={},members={};
    function vlan(vid) {if(!vs[vid])vs[vid]={vid,members:[],untagged:[],cpu:false};return vs[vid];}
    for(let row in s.vlans) {
        let id=portid(row.ifname),isSelf=bridge&&row.ifname==bridge.name;
        if(!isSelf&&(!id||id>count||ports[id-1].master!=bridge?.name))continue;
        for(let entry in row.vlans || []) {
            let last=entry.vlanEnd || entry.vlan_end || entry.vlan;
            for(let vid=int(entry.vlan);vid<=int(last);vid++) {
                if(vid<1||vid>=4031)die('VLANs 4031-4094 are reserved for standalone ports');
                let v=vlan(vid);
                if(isSelf){v.cpu=true;self[vid]=entry.flags || [];continue;}
                push(v.members,id);members[id+':'+vid]=true;
                if(has(entry.flags,'Egress Untagged'))push(v.untagged,id);
                if(has(entry.flags,'PVID'))ports[id-1].pvid=vid;
            }
        }
    }
    for(let p in ports) {
        if(!p.master) {let v=vlan(p.pvid);v.cpu=true;push(v.members,p.id);push(v.untagged,p.id);members[p.id+':'+p.pvid]=true;}
        else if(p.pvid>=4031)die(p.name+' needs an ingress PVID in the supported bridge configuration');
    }
    let ifmap={},locals={},rifs={};
    function local(vid,m) {locals[vid+':'+m]={vid,mac:m};}
    for(let p in ports) {
        if(!p.master)ifmap[p.name]={vid:p.pvid,mac:p.mac,port:p.id};
        if(vs[p.pvid]?.cpu)local(p.pvid,p.mac);
    }
    if(bridge) {
        for(let vid,flags in self) {local(int(vid),bridge.mac);if(has(flags,'PVID'))ifmap[bridge.name]={vid:int(vid),mac:bridge.mac};}
        for(let l in s.links) if(l.linkinfo?.info_kind=='vlan') {
            let parent=l.link || indices[l.link_index];
            if(parent==bridge.name) {
                let vid=int(l.linkinfo.info_data.id);
                if(!vs[vid]?.cpu)die(l.ifname+' has no local bridge VLAN membership');
                ifmap[l.ifname]={vid,mac:mac(l.address)};local(vid,mac(l.address));
            } else if(portid(parent))die('use bridge VLAN interfaces rather than per-port VLAN uppers');
        }
    }
    let localIPs=[];
    for(let a in s.addresses) {
        let info=ifmap[a.ifname];
        if(portid(a.ifname)&&ports[portid(a.ifname)-1]?.master&&length(a.addr_info || []))die('assign addresses to the bridge VLAN interface, not an enslaved switch port');
        if(!info)continue;
        for(let addr in a.addr_info || []) if(addr.family=='inet') {
            if((links[a.ifname]?.mtu || 1500)!=1500)die('routed MTU above 1500 is not supported yet');
            rifs[info.vid]={vid:info.vid,mac:info.mac};local(info.vid,info.mac);
            push(localIPs,{ip:ipnum(addr.local),mask:0xffffffff,bits:32,kind:'local',priority:100});
        }
    }
    let staticFDB=[],lookup={},staticKeys={};
    for(let f in s.fdb) {
        let p=portid(f.ifname || f.dev);if(!p||p>count||!ports[p-1].master||!f.mac)continue;
        let vid=int(f.vlan || ports[p-1].pvid),m=mac(f.mac);
        if(!members[p+':'+vid])continue;
        if(m==ports[p-1].mac||m==bridge?.mac)continue;
        if(has(f.flags,'self'))continue;
        lookup[vid+':'+m]=p;
        let state=type(f.state)=='array'?f.state:[f.state];
        if((has(state,'permanent')||has(state,'static'))&&!has(f.flags,'extern_learn')) {staticKeys[vid+':'+m]=true;push(staticFDB,{vid,mac:m,port:p});}
    }
    for(let line in split(s.hardware_fdb || '', '\n')) {
        let m=match(line,/^mac=([^ ]+) vlan=([0-9]+).*port=([0-9]+)\/xe/);
        if(m&&!staticKeys[m[2]+':'+m[1]])lookup[m[2]+':'+m[1]]=int(m[3]);
    }
    let neighbors={};
    for(let n in s.neighbors) {
        let info=ifmap[n.dev];if(!info||!rifs[info.vid]||!n.lladdr||has(n.state,'FAILED')||has(n.state,'INCOMPLETE'))continue;
        let m=mac(n.lladdr),p=info.port || lookup[info.vid+':'+m];
        if(p&&members[p+':'+info.vid]&&ports[p-1].admin&&ports[p-1].state==4)neighbors[n.dev+':'+n.dst]={vid:info.vid,port:p,mac:m};
    }
    /* Priorities alone do not identify Linux's default rule chain. A rule
       at 32766 may select another table, an input interface or an fwmark. */
    let policy=length(s.rules || [])!=3,seenRules={};
    let defaultTables={'0':['local',255],'32766':['main',254],'32767':['default',253]};
    for(let rule in s.rules || []) {
        let key=''+rule.priority,allowed=defaultTables[key];
        if(!allowed||seenRules[key]||!has(allowed,rule.table)||(rule.src || 'all')!='all')policy=true;
        for(let field in keys(rule)) if(!has(['priority','src','table','protocol'],field))policy=true;
        seenRules[key]=true;
    }
    /* TOS/source selectors cannot be represented by our destination-only LPM
       keys. Avoid installing a broad hardware route that bypasses them. */
    for(let r in s.routes)if((r.tos&&r.tos!='0x00')||(r.from&&r.from!='all'))policy=true;
    if(length(keys(s.firewall || {})))policy=true;
    let warnings=[];if(policy)push(warnings,'IPv4 hardware routing bypassed: policy rules or firewall configuration require CPU forwarding');
    let routes={},nhops={};
    function route(r) {let key=r.ip+'/'+r.bits;if(!routes[key]||r.priority>routes[key].priority)routes[key]=r;}
    if(length(keys(rifs)))route({ip:0,mask:0,bits:0,kind:'cpu',priority:0});
    let selected={};
    for(let r in s.routes) {
        let info=ifmap[r.dev];if(!length(keys(rifs)))continue;
        let p=prefix(r.dst || 'default'),key=p.ip+'/'+p.bits;
        if(selected[key]&&(selected[key].metric || 0)<(r.metric || 0))continue;
        if(selected[key]&&(selected[key].metric || 0)==(r.metric || 0)&&(selected[key].gateway!=r.gateway||selected[key].dev!=r.dev))r={...r,nexthops:['ambiguous']};
        selected[key]=r;
    }
    for(let key,r in selected) {
        let p=prefix(r.dst || 'default'),info=ifmap[r.dev],nh=null;
        if(!policy&&s.forwarding&&(r.type || 'unicast')=='unicast'&&r.gateway&&!r.nexthops&&!r.nhid&&!r.encap&&!r.metrics&&!has(r.flags,'linkdown')&&!has(r.flags,'dead')&&info&&rifs[info.vid])nh=neighbors[r.dev+':'+r.gateway];
        if(nh){let k=nh.vid+':'+nh.port+':'+nh.mac;nhops[k]=nh;route({...p,kind:'hardware',nh:k,priority:20});}
        else route({...p,kind:'cpu',priority:20});
    }
    if(!policy&&s.forwarding)for(let n in s.neighbors) {
        let nh=neighbors[n.dev+':'+n.dst];if(!nh)continue;
        let addr=ipnum(n.dst),best=null;
        for(let key,r in selected) {let p=prefix(r.dst || 'default');if((addr&p.mask)==p.ip&&(!best||p.bits>best.bits))best={...p,route:r};}
        if(!best||best.route.gateway||best.route.dev!=n.dev||(best.route.type || 'unicast')!='unicast'||best.route.nexthops||best.route.nhid||best.route.encap||best.route.metrics||has(best.route.flags,'linkdown')||has(best.route.flags,'dead'))continue;
        let k=nh.vid+':'+nh.port+':'+nh.mac;nhops[k]=nh;
        route({ip:addr,mask:0xffffffff,bits:32,kind:'hardware',nh:k,priority:30});
    }
    for(let a in localIPs)route(a);
    let vlanList=sort(objarray(vs),(a,b)=>a.vid-b.vid);
    for(let v in vlanList){sort(v.members);sort(v.untagged);}
    let localList=sort(objarray(locals),(a,b)=>sprintf('%04d%s',a.vid,a.mac)<sprintf('%04d%s',b.vid,b.mac)?-1:1);
    let rifList=sort(objarray(rifs),(a,b)=>a.vid-b.vid);
    let nhList=[];for(let k in sort(keys(nhops)))push(nhList,{...nhops[k],key:k});
    let routeList=sort(objarray(routes),(a,b)=>a.bits-b.bits || a.ip-b.ip);
    if(length(rifList)>128||length(nhList)>1000||length(routeList)>1000||length(localList)>512||length(staticFDB)>512)die('native controller object limit exceeded');
    sort(staticFDB,(a,b)=>sprintf('%04d%s',a.vid,a.mac)<sprintf('%04d%s',b.vid,b.mac)?-1:1);
    let topology={generation:s.generation || "legacy",profile,bridge,vlans:vlanList,local_macs:localList,static_fdb:staticFDB,pvids:map(ports,p=>({id:p.id,pvid:p.pvid,master:p.master}))};
    return {profile,count,bridge,ports,topology,rifs:rifList,nhops:nhList,routes:routeList,warnings,forwarding:s.forwarding};
}
function compile(p,old,nonce) {
    let full=!old || sprintf('%J',p.topology)!=sprintf('%J',old.topology);
    let l3=full || sprintf('%J',[p.rifs,p.nhops,p.routes])!=sprintf('%J',[old.rifs,old.nhops,old.routes]);
    let a=['int native_apply_linux() {','int rv;','int nh[1024];','bcm_mac_t m;','bcm_pbmp_t members;','bcm_pbmp_t untagged;'];
    function emit(s){push(a,s);}
    function check(expr,label){emit('rv='+expr+'; if(rv<0) { printf("NATIVE_ERROR '+label+' %d\\n",rv); return rv; }');}
    function setmac(m){let v=split(m,':');for(let i=0;i<6;i++)emit('m['+i+']=0x'+v[i]+';');}
    if(full)for(let port in p.ports)check('bcm_port_enable_set(0,'+port.id+',0)','disable-port');
    if(full)check('native_clear_l3()','clear-l3');
    if(l3)check('native_begin_l3()','begin-l3');
    if(full) {
        check('native_clear_l2()','clear-l2');
        for(let v in p.topology.vlans) {
            emit('BCM_PBMP_CLEAR(members); BCM_PBMP_CLEAR(untagged);');
            if(v.cpu)emit('BCM_PBMP_PORT_ADD(members,0);');
            for(let id in v.members)emit('BCM_PBMP_PORT_ADD(members,'+id+');');
            for(let id in v.untagged)emit('BCM_PBMP_PORT_ADD(untagged,'+id+');');
            check('native_vlan('+v.vid+',members,untagged)','vlan-'+v.vid);
        }
    }
    if(full)for(let m in p.topology.local_macs) {setmac(m.mac);check('native_local_mac('+m.vid+',m)','local-mac');}
    if(full)for(let f in p.topology.static_fdb) {setmac(f.mac);check('native_static_mac('+f.vid+',m,'+f.port+')','static-fdb');}
    if(l3) {
        for(let rif in p.rifs) {setmac(rif.mac);check('native_rif('+rif.vid+',m)','rif-'+rif.vid);}
        if(length(p.routes))check('native_cpu_egress()','cpu-egress');
        for(let i=0;i<length(p.nhops);i++) {
            let nh=p.nhops[i];setmac(nh.mac);
            emit('nh['+i+']=native_nexthop('+nh.vid+','+nh.port+',m); if(nh['+i+']<0) { printf("NATIVE_ERROR next-hop %d\\n",nh['+i+']); return nh['+i+']; }');
        }
        for(let r in p.routes) {
            let egr='native_cpu';if(r.kind=='hardware')for(let i=0;i<length(p.nhops);i++)if(p.nhops[i].key==r.nh)egr='nh['+i+']';
            check(sprintf('native_route(0x%08x,0x%08x,%s)',r.ip,r.mask,egr),'route');
        }
    }
    if(l3)check('native_finish_l3()','finish-l3');
    for(let port in p.ports)if(full||sprintf('%J',port)!=sprintf('%J',old.ports[port.id-1]))check('native_port('+join(',',[port.id,port.pvid,port.admin?1:0,port.speed,port.autoneg,port.state,port.learning?1:0])+')','port-'+port.id);
    check('native_report()','metrics');
    emit('printf("NATIVE_APPLY_OK_'+nonce+'\\n"); return 0; }');emit('print native_apply_linux();');
    return join('\n',a)+'\n';
}
function state_status() {
    let st=load(ROOT+'/native-status.json',{ok:false,message:'waiting for native networking'});
    st.running=!!fs.stat('/var/run/bcm-sdk.sock');st.initialized=read('/var/run/bcm-sdk.ready','')=='ready';
    st.applying=!!fs.stat(LOCK);st.last_apply_ok=st.ok;st.last_apply=st.message;
    st.watcher=load(ROOT+'/watcher-status.json',null);
    st.chip='BCM56845_A3';st.profile=read(ROOT+'/active-profile','40g');
    st.ports=st.initialized&&!st.applying?sdk('ps'):'';
    return st;
}
function apply_once(force) {
    if(read('/var/run/bcm-sdk.ready','')!='ready')return {ok:false,message:'ASIC has not initialized'};
    /* The kernel releases flock on process death. The directory is only a
       marker for the SDK/TAP worker, never the mutual-exclusion primitive. */
    let guard=fs.open(LOCK+'.lock','ae');
    if(!guard||!guard.lock('xn')) {
        if(guard)guard.close();
        return {ok:false,message:'another native reconcile is running'};
    }
    if(!fs.mkdir(LOCK)) {
        fs.unlink(LOCK+'/pid');fs.rmdir(LOCK);
        if(!fs.mkdir(LOCK)) {guard.close();return {ok:false,message:'cannot create native apply marker'};}
    }
    fs.writefile(LOCK+'/pid',fs.readlink('/proc/self'));
    let result,planChanged=false,applied=null;
    try {
        let p=plan(snapshot());
        if(p.profile!=read(ROOT+'/active-profile',''))die('lane profile change needs ASIC restart');
        if(read('/sys/module/tun/parameters/bcm_switch_offload','')!='Y')die('SDK bridge forwarding protection is not enabled');
        if(p.bridge)for(let port in p.ports)if(port.master&&read('/sys/class/net/'+port.name+'/phys_switch_id','')!='62636d3536383435')die('missing hardware bridge identity for '+port.name);
        let old=load(ROOT+'/native-plan.json',null);
        if(!force&&fs.stat(ROOT+'/native-current.json')&&!fs.stat(ROOT+'/native-failed')&&sprintf('%J',old)==sprintf('%J',p))result=load(ROOT+'/native-status.json',{ok:true,message:'native state unchanged'});
        else {
            planChanged=true;
            let nonce=time()+'_'+fs.readlink('/proc/self')+'_'+(++transaction);let code=compile(p,(force||fs.stat(ROOT+'/native-failed'))?null:old,nonce);
            if(!fs.writefile(ROOT+'/native-failed','apply pending '+nonce+'\n'))die('cannot mark pending ASIC update');
            if(!fs.writefile(ROOT+'/native-apply.cint',code))die('cannot stage ASIC update');
            let output=sdk('cint '+ROOT+'/native-apply.cint');
            fs.writefile(ROOT+'/native-last-sdk.txt',output);
            if(index(output,'NATIVE_APPLY_OK_'+nonce)<0)die('ASIC rejected native configuration: '+substr(output,-1200));
            let metrics=match(output,/NATIVE_METRICS ([^\r\n]+)/);
            let objects=metrics?json(metrics[1]):null;
            write(ROOT+'/native-plan.json',p);applied=p;
            result={ok:true,message:'Native Linux networking applied to ASIC',timestamp:time(),
                source:'Linux/netifd',bridge:p.bridge?.name || null,vlan_count:length(p.topology.vlans),
                rif_count:length(p.rifs),hardware_routes:length(filter(p.routes,r=>r.kind=='hardware')),
                cpu_routes:length(filter(p.routes,r=>r.kind!='hardware')),warnings:p.warnings,objects};
        }
    } catch(e) {
        /* Linux may already have changed topology: keep an incompatible ASIC
           snapshot from continuing to forward while the user fixes the error. */
        fs.writefile(ROOT+'/native-failed','1\n');
        try {sdk('port xe enable=false');}catch(ignored){}
        result={ok:false,message:e.message || ''+e,timestamp:time(),source:'Linux/netifd'};
        planChanged=true;
    }
    if(planChanged){write(ROOT+'/native-status.json',result);write(ROOT+'/last-apply.json',result);
        write(ROOT+'/native-current.json',{status:result,plan:applied || load(ROOT+'/native-plan.json',null)});}
    if(result.ok)fs.unlink(ROOT+'/native-failed');
    fs.unlink(LOCK+'/pid');fs.rmdir(LOCK);
    guard.close();
    return result;
}
let mode=ARGV[0] || 'status';
if(mode=='plan')print(sprintf('%J\n',plan(snapshot())));
else if(mode=='fixture')print(sprintf('%J\n',plan(json(fs.readfile(ARGV[1])))));
else if(mode=='status')print(sprintf('%J\n',state_status()));
else if(mode=='apply'){let r=apply_once(false);print(sprintf('%J\n',r));if(!r.ok)exit(1);}
else if(mode=='reconcile'){let r=apply_once(true);print(sprintf('%J\n',r));if(!r.ok)exit(1);}
else if(mode=='daemon'){while(true){apply_once(false);sleep(2000);}}
else die('usage: bcm-native.uc {status|plan|apply|daemon|fixture FILE}');
