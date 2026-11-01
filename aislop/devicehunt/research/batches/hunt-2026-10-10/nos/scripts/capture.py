import hashlib,json,os,shutil,sys,subprocess
N='/tmp/claude-1001/-home-erin-nepeat/2a56d3f5-4097-451b-b4cf-73082bce3b29/scratchpad/nos'
OUT='/home/erin/nepeat/aislop/devicehunt/data/raw/hunt-2026-10-10-nos'
times=dict(l.split() for l in open(N+'/clone-times.txt'))
repos=[('sonic-net/sonic-buildimage','sb','sb-match.txt'),('opencomputeproject/OpenNetworkLinux','onl','onl-match.txt')]
files=[];meta=[]
for full,d,m in repos:
    c=subprocess.check_output(['git','-C',f'{N}/{d}','rev-parse','HEAD'],text=True).strip()
    name=full.split('/')[1]
    cnt=0
    for p in sorted(open(f'{N}/{m}').read().split()):
        src=f'{N}/{d}/{p}'; dst=f'{OUT}/{name}/{c}/{p}'
        os.makedirs(os.path.dirname(dst),exist_ok=True)
        b=open(src,'rb').read(); open(dst,'wb').write(b)
        files.append({'repo':full,'commit':c,'path':p,'local':f'{name}/{c}/{p}','sha256':hashlib.sha256(b).hexdigest(),'bytes':len(b),'permalink':f'https://github.com/{full}/blob/{c}/{p}'}); cnt+=1
    meta.append({'repo':full,'commit':c,'captured_utc':times[d],'method':'blobless git clone (sonic: fetch --depth 1 of commit; ONL: --depth 1), sparse checkout of matched paths only','file_count':cnt})
json.dump({'captured_by':'nos_platform_code','repos':meta,'files':files},open(OUT+'/capture-manifest.json','w'),indent=1)
print(len(files))
