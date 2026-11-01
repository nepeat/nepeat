import sys,subprocess,hashlib,json,time,os,datetime
R='/home/erin/nepeat/aislop/devicehunt/data/raw/discovery-2026-10-11-spec'
LOG=R+'/wave10/requests-wave10.jsonl'
UA="devicehunt-research/1.0 (non-commercial hardware identification research; low-rate)"
def n():
    return sum(1 for _ in open(LOG)) if os.path.exists(LOG) else 0
def fetch(url,rel,follow=False):
    if n()>=150: raise SystemExit('cap')
    if os.path.exists(LOG):
        last=json.loads(open(LOG).read().splitlines()[-1])
        if last['status']==429: raise SystemExit('429 stop')
    p=os.path.join(R,rel); os.makedirs(os.path.dirname(p),exist_ok=True)
    time.sleep(10)
    t=datetime.datetime.now(datetime.timezone.utc).strftime('%Y-%m-%dT%H:%M:%SZ')
    cmd=['curl','-sS','-A',UA,'-o',p,'-w','%{http_code}']+(['-L','--max-redirs','3'] if follow else [])+[url]
    r=subprocess.run(cmd,capture_output=True,text=True)
    st=int(r.stdout or 0)
    rec={'url':url,'status':st,'utc':t,'path':rel if os.path.exists(p) else None}
    if os.path.exists(p):
        b=open(p,'rb').read(); rec['sha256']=hashlib.sha256(b).hexdigest(); rec['bytes']=len(b)
        if st!=200 and st!=302: pass
    rec['err']=r.stderr.strip() or None
    open(LOG,'a').write(json.dumps(rec)+'\n')
    return rec
if __name__=='__main__':
    print(fetch(sys.argv[1],sys.argv[2],len(sys.argv)>3))
