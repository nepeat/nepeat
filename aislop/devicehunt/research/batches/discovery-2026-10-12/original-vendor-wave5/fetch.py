"""One counted authorized HTTP exchange; no redirects, retry, crawl or resume run."""
import argparse
import datetime as dt
import hashlib
from html.parser import HTMLParser
import json
from pathlib import Path
import re
import subprocess
import time
from urllib import robotparser
from urllib.parse import urljoin, urlsplit

HERE=Path(__file__).resolve().parent
ROOT=HERE.parents[3]
PLAN=HERE.parent/'next-leads-r2/plan.json'
UA='devicehunt-public-research/1.0'
POLICIES={'oracle':('https://docs.oracle.com/robots.txt','robots-oracle.txt'),
          'as6700':('https://www.edge-core.com/robots.txt','robots-edgecore.txt'),
          'as5712':('https://www.edge-core.com/robots.txt','robots-edgecore.txt')}
MAX={'oracle':3,'as6700':5,'as5712':5}
GROUPS={'oracle':1,'as6700':2,'as5712':3}


def sha(body): return hashlib.sha256(body).hexdigest()


class Links(HTMLParser):
    def __init__(self,text):
        super().__init__(convert_charrefs=True)
        self.links=[]
        self.feed(text)
    def handle_starttag(self,tag,attrs):
        if tag=='a' and dict(attrs).get('href'):
            self.links.append(dict(attrs)['href'])


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('group',choices=GROUPS)
    parser.add_argument('url')
    parser.add_argument('output')
    parser.add_argument('--policy',action='store_true')
    parser.add_argument('--parent',type=int)
    args=parser.parse_args()
    plan_body=PLAN.read_bytes()
    assert sha(plan_body)=='c7ff2b1bd6b21bfc8204742174539566fa90172a4fc58e09b381d68d63ce3931'
    plan=json.loads(plan_body)
    lead=next(p for p in plan['leads'] if p['priority']==GROUPS[args.group])
    assert sha((ROOT/lead['basis']).read_bytes())==lead['basis_sha256']
    for name,expected in plan['prior_input_hashes'].items():
        assert sha((ROOT/name).read_bytes())==expected,name
    ledger_path=HERE/'requests.jsonl'
    records=[json.loads(line) for line in ledger_path.read_text().splitlines()] if ledger_path.exists() else []
    assert len(records)<12 and not any(r['status']==429 for r in records),'cap or throttle stops run'
    assert args.url not in {r['url'] for r in records},'no repeat request'
    policy_url,policy_name=POLICIES[args.group]
    host=urlsplit(policy_url).netloc
    assert urlsplit(args.url).scheme=='https' and urlsplit(args.url).netloc==host,'unexpected origin'
    relevant=[r for r in records if r['group']==args.group and not r['is_policy']]
    shared_policy=[r for r in records if r['url']==policy_url]
    assert len(relevant)+len(shared_policy)<MAX[args.group]
    assert not any(r['status'] in (0,402,403,404,429) or r['login_marker'] or r['curl_returncode'] for r in relevant),'source already stopped'
    authorization={}
    policy=None
    delay=0
    if args.policy:
        assert args.url==policy_url and args.output==policy_name and not shared_policy
        authorization={'kind':'fresh host policy','plan_sha256':sha(plan_body)}
    else:
        assert len(shared_policy)==1 and shared_policy[0]['status']==200 and not shared_policy[0]['curl_returncode'] and not shared_policy[0]['login_marker']
        body=(HERE/policy_name).read_bytes()
        assert sha(body)==shared_policy[0]['body_sha256']
        robots=robotparser.RobotFileParser()
        robots.parse(body.decode('utf-8').splitlines())
        allowed=robots.can_fetch(UA,args.url)
        delay=robots.crawl_delay(UA) or robots.crawl_delay('*') or 0
        assert allowed,'fresh robots prohibits exact path'
        assert delay<=60,'longer policy delay requires checkpoint'
        policy={'path':policy_name,'sha256':sha(body),'user_agent':UA,'exact_URL':args.url,'exact_path_allowed':allowed,'crawl_delay_seconds':delay}
        if args.url==lead['first_fetch']:
            assert args.parent is None
            authorization={'kind':'planned canonical landing/page','plan_sha256':sha(plan_body),'basis':lead['basis'],'basis_sha256':lead['basis_sha256']}
        else:
            assert args.parent is not None
            parent=next(r for r in records if r['sequence']==args.parent)
            assert parent['group']==args.group
            if parent['status'] in (301,302,303,307,308):
                expected=urljoin(parent['url'],parent['location_not_followed'])
                assert args.url==expected,'only exact saved Location permitted'
                if args.group=='oracle':
                    assert urlsplit(args.url).path.endswith('sun-zfs-storage-7420-specifications.html')
                authorization={'kind':'manual saved redirect','parent_sequence':args.parent,'parent_header_sha256':parent['header_sha256'],'literal_Location':parent['location_not_followed']}
            else:
                assert args.group!='oracle' and parent['url']==lead['first_fetch'] and parent['status']==200
                links=Links((HERE/parent['body_path']).read_text()).links
                assert args.url in [urljoin(parent['url'],href) for href in links]
                assert not any(r.get('authorization',{}).get('kind')=='fresh published download' for r in relevant)
                authorization={'kind':'fresh published download','parent_sequence':args.parent,'parent_body_sha256':parent['body_sha256'],'exact_href':next(h for h in links if urljoin(parent['url'],h)==args.url)}
    previous=[r for r in records if urlsplit(r['url']).netloc==host]
    wait=0
    if previous:
        elapsed=(dt.datetime.now(dt.timezone.utc)-dt.datetime.fromisoformat(previous[-1]['finished_at_utc'])).total_seconds()
        wait=max(0,delay-elapsed)
    if wait:time.sleep(wait)
    output=HERE/args.output
    headers=HERE/(args.output+'.headers')
    assert output.parent==HERE and not output.exists() and not headers.exists(),'immutable new capture filenames only'
    argv=['curl','--silent','--show-error','--max-time','45','--retry','0','--user-agent',UA,'--dump-header',str(headers),'--output',str(output),'--write-out','%{http_code}',args.url]
    started=dt.datetime.now(dt.timezone.utc).isoformat()
    result=subprocess.run(argv,capture_output=True,text=True)
    finished=dt.datetime.now(dt.timezone.utc).isoformat()
    status=int(result.stdout) if re.fullmatch(r'\d{3}',result.stdout) else 0
    body=output.read_bytes() if output.exists() else b''
    header_bytes=headers.read_bytes() if headers.exists() else b''
    header_text=header_bytes.decode('utf-8',errors='replace')
    location=next((line.split(':',1)[1].strip() for line in header_text.splitlines() if line.lower().startswith('location:')),None)
    login=bool(re.search(rb'<input[^>]+type=["\']?password|<title>[^<]*(?:sign in|log in|login)',body,re.I))
    record=dict(sequence=len(records)+1,group=args.group,is_policy=args.policy,method='GET',url=args.url,started_at_utc=started,finished_at_utc=finished,user_agent=UA,argv=argv,
                status=status,curl_returncode=result.returncode,curl_stderr=result.stderr,automatic_redirects=False,retries=0,location_not_followed=location,login_marker=login,
                body_path=args.output if output.exists() else None,body_bytes=len(body),body_sha256=sha(body) if output.exists() else None,
                header_path=headers.name if headers.exists() else None,header_sha256=sha(header_bytes) if headers.exists() else None,
                policy=policy,authorization=authorization,seconds_waited_for_policy=wait)
    with ledger_path.open('a') as handle:handle.write(json.dumps(record)+'\n')
    print(json.dumps(record,indent=2))


if __name__=='__main__':main()
