import os,sys,json,hashlib,shutil,subprocess
repo,raw=sys.argv[1],sys.argv[2]
commit="c431b65bf2905ce88d14578a03c68338413b353b"
t=open(os.path.join(os.path.dirname(repo),'clonetime')).read().strip()
lic=subprocess.run(['git','-C',repo,'show','HEAD:LICENSE'],capture_output=True,text=True).stdout
files=[]
for dp,dn,fn in os.walk(os.path.join(repo,'device')):
    if os.path.basename(dp)=='pddf' and not os.path.islink(dp):
        for f in fn:
            files.append(os.path.relpath(os.path.join(dp,f),repo))
files.sort(); man=[]
for rel in files:
    dst=os.path.join(raw,rel); os.makedirs(os.path.dirname(dst),exist_ok=True)
    shutil.copyfile(os.path.join(repo,rel),dst)
    b=open(dst,'rb').read()
    man.append({"path":rel,"sha256":hashlib.sha256(b).hexdigest(),"bytes":len(b),
      "permalink":f"https://github.com/sonic-net/sonic-buildimage/blob/{commit}/{rel}"})
json.dump({"repo":"https://github.com/sonic-net/sonic-buildimage","commit":commit,
 "clone_utc":t,"method":"git init + remote add; git fetch --depth 1 --filter=blob:none origin <commit hash> (single commit fetched by hash, accepted by server); sparse-checkout cone set device/{accton,alphanetworks,celestica,ingrasys,inventec,quanta,wistron}; git checkout FETCH_HEAD",
 "license":"Apache-2.0 (repo LICENSE: "+lic.strip().splitlines()[0].strip()+")","files":man},open(os.path.join(raw,'capture-manifest.json'),'w'),indent=1)
print(len(man))
