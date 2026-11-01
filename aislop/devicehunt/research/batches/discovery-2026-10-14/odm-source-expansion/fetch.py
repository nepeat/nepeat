"""Single counted planned exchange; no redirect/retry/crawl/import."""
import argparse, datetime as dt, hashlib, json, re, subprocess, time
from pathlib import Path
from urllib import robotparser
from urllib.parse import urlsplit,urljoin
HERE=Path(__file__).resolve().parent
UA='devicehunt-public-research/1.0'
def sha(b):return hashlib.sha256(b).hexdigest()
def main():
    ap=argparse.ArgumentParser(description=__doc__)
    ap.add_argument('key');ap.add_argument('--policy',action='store_true');ap.add_argument('--parent',type=int)
    args=ap.parse_args()
    assert not (HERE/'capture-complete.json').exists(),'finite set closed'
    plan_bytes=(HERE/'plan.json').read_bytes();plan=json.loads(plan_bytes)
    resource=next(r for r in plan['resource_routes'] if r['key']==args.key)
    ledger=HERE/'requests.jsonl'
    records=[json.loads(x) for x in ledger.read_text().splitlines()] if ledger.exists() else []
    assert len(records)<12 and not any(r['status']==429 for r in records),'budget/throttle stop'
    host=urlsplit(resource['url']).netloc;policy_url='https://'+host+'/robots.txt'
    policies=[r for r in records if r['url']==policy_url]
    previous=[r for r in records if urlsplit(r['url']).netloc==host]
    assert not any(r['status'] in (0,402,403,404,429) or r['gate_marker'] or r['curl_returncode'] for r in previous),'host stopped'
    policy=None;delay=0;authorization={'plan_sha256':sha(plan_bytes),'resource_key':args.key}
    if args.policy:
        assert not policies and args.parent is None
        url=policy_url;output='robots-'+host+'.txt';authorization['kind']='fresh policy'
    else:
        assert len(policies)==1 and policies[0]['status']==200 and not policies[0]['curl_returncode'],'policy unavailable'
        p=policies[0];body=(HERE/p['body_path']).read_bytes();assert sha(body)==p['body_sha256']
        robots=robotparser.RobotFileParser();robots.parse(body.decode('utf-8').splitlines())
        if args.parent is None:
            url=resource['url'];output=resource['output'];authorization['kind']='exact planned bulk resource'
        else:
            parent=next(r for r in records if r['sequence']==args.parent)
            assert parent['key']==args.key and parent['status'] in (301,302,303,307,308) and parent['authorization']['kind']=='exact planned bulk resource'
            url=urljoin(parent['url'],parent['location_not_followed']);output='redirect-'+resource['output']
            assert urlsplit(url).netloc==host and urlsplit(url).scheme=='https','unexpected redirect origin'
            authorization.update(kind='literal same-origin redirect',parent_sequence=args.parent,literal_Location=parent['location_not_followed'])
        assert robots.can_fetch(UA,url),'fresh policy denies exact path'
        delay=robots.crawl_delay(UA) or robots.crawl_delay('*') or 0
        assert delay<=60,'long-delay checkpoint required'
        policy={'body_path':p['body_path'],'sha256':p['body_sha256'],'user_agent':UA,'exact_URL':url,'allowed':True,'crawl_delay_seconds':delay}
    assert url not in {r['url'] for r in records},'no repeat request'
    path=HERE/output;headers=HERE/(output+'.headers');assert not path.exists() and not headers.exists()
    wait=0
    if previous:wait=max(0,delay-(dt.datetime.now(dt.timezone.utc)-dt.datetime.fromisoformat(previous[-1]['finished_at_utc'])).total_seconds())
    if wait:time.sleep(wait)
    argv=['curl','--silent','--show-error','--max-time','45','--connect-timeout','15','--max-filesize','20000000','--retry','0','--max-redirs','0','--user-agent',UA,'--dump-header',str(headers),'--output',str(path),'--write-out','%{http_code}',url]
    start=dt.datetime.now(dt.timezone.utc).isoformat();r=subprocess.run(argv,capture_output=True,text=True);finish=dt.datetime.now(dt.timezone.utc).isoformat()
    status=int(r.stdout) if re.fullmatch(r'\d{3}',r.stdout) else 0
    body=path.read_bytes() if path.exists() else b'';hb=headers.read_bytes() if headers.exists() else b''
    location=next((s.split(':',1)[1].strip() for s in hb.decode('utf-8',errors='replace').splitlines() if s.lower().startswith('location:')),None)
    gate=bool(re.search(rb'<input[^>]+type=["\x27]?password|<title>[^<]*(?:sign in|log in|login|registration|access denied)',body,re.I))
    record=dict(sequence=len(records)+1,key=args.key,dataset=resource['dataset'],is_policy=args.policy,url=url,method='GET',started_at_utc=start,finished_at_utc=finish,status=status,user_agent=UA,argv=argv,curl_returncode=r.returncode,curl_stderr=r.stderr,automatic_redirects=False,retries=0,gate_marker=gate,location_not_followed=location,body_path=path.name if path.exists() else None,body_bytes=len(body),body_sha256=sha(body) if path.exists() else None,header_path=headers.name if headers.exists() else None,header_sha256=sha(hb) if headers.exists() else None,policy=policy,authorization=authorization,seconds_waited_for_policy=wait)
    with ledger.open('a') as f:f.write(json.dumps(record)+'\n')
    print(json.dumps({k:record[k] for k in ('sequence','key','url','status','body_bytes','body_sha256','location_not_followed','gate_marker','curl_returncode','curl_stderr')},indent=2))
if __name__=='__main__':main()
