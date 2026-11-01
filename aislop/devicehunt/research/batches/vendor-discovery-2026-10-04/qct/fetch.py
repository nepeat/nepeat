import urllib.request,urllib.error,json,hashlib,pathlib,datetime,sys
p=pathlib.Path('data/raw/vendor-discovery-qct-2026-10-04');out=json.loads((p/'fetch-report.json').read_text())
for key,url in json.loads(sys.argv[1]).items():
 try:r=urllib.request.urlopen(url,timeout=35)
 except urllib.error.HTTPError as e:r=e
 b=r.read();f=p/key;f.write_bytes(b)
 rec={'key':key,'url':url,'final_url':r.url,'status':r.code,'capture_utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'path':str(f),'sha256':hashlib.sha256(b).hexdigest(),'bytes':len(b)};out.append(rec);print(rec)
(p/'fetch-report.json').write_text(json.dumps(out,indent=2))
