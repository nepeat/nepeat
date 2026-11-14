#!/usr/bin/env python3
"""Exercise real reconcile lock recovery without changing forwarding intent."""
from pathlib import Path
import datetime,json,subprocess,time
ROOT=Path(__file__).resolve().parent.parent
SSH=['bash',str(ROOT/'ssh.sh')]
def call(command):return subprocess.run(SSH+[command],capture_output=True,text=True,timeout=30)
def main():
    record={'date_utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),
            'controller_sha256':call('sha256sum /usr/libexec/bcm-native.uc').stdout.split()[0]}
    child=None;owner=None
    call('/etc/init.d/bcm-network stop').check_returncode()
    try:
        call('rm -f /tmp/native-lock-held').check_returncode()
        for label,pid in [('empty',''),('dead_pid','99999999'),('reused_pid','1')]:
            command='mkdir -p /var/lock/bcm-switch-apply; '+('echo '+pid+' > /var/lock/bcm-switch-apply/pid; ' if pid else '')+'/usr/libexec/bcm-switch apply'
            p=call(command);record[label]={'rc':p.returncode,'result':p.stdout};assert not p.returncode,p.stdout+p.stderr
        holder="import * as fs from 'fs'; let g=fs.open('/var/lock/bcm-switch-apply.lock','ae');if(!g.lock('xn'))exit(1);fs.writefile('/tmp/native-lock-held',fs.readlink('/proc/self'));sleep(30000);"
        subprocess.run(SSH+['cat > /tmp/native-lock-holder.uc'],input=holder,text=True,check=True)
        child=subprocess.Popen(SSH+['ucode /tmp/native-lock-holder.uc'],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
        for _ in range(30):
            p=call('cat /tmp/native-lock-held')
            if not p.returncode:break
            time.sleep(.1)
        owner=int(p.stdout.strip())
        p=call('/usr/libexec/bcm-switch apply');record['contended']={'rc':p.returncode,'result':p.stdout};assert p.returncode and 'another native reconcile' in p.stdout
        call('kill -9 '+str(owner)).check_returncode();child.wait(timeout=10);owner=None
        p=call('/usr/libexec/bcm-switch apply');record['after_owner_killed']={'rc':p.returncode,'result':p.stdout};assert not p.returncode,p.stdout+p.stderr
        # Interrupt a real apply after its dirty marker appears. The SDK
        # command may finish independently; the cache must not suppress retry.
        apply_child=subprocess.Popen(SSH+['/usr/libexec/bcm-switch reconcile'],stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True)
        interrupted=None
        for _ in range(100):
            p=call('test -f /tmp/bcm-sdk/native-failed && cat /var/lock/bcm-switch-apply/pid')
            if p.returncode==0 and p.stdout.strip():
                interrupted=int(p.stdout.strip());break
            if apply_child.poll() is not None:break
            time.sleep(.02)
        assert interrupted,'missed pending SDK update'
        call('kill -9 '+str(interrupted)).check_returncode()
        stdout,stderr=apply_child.communicate(timeout=30)
        assert apply_child.returncode,('apply finished before interruption',stdout,stderr)
        p=call('cat /tmp/bcm-sdk/native-failed');assert p.returncode==0,p.stderr
        record['interrupted_apply']={'rc':apply_child.returncode,'pending_marker':p.stdout}
        p=call('/usr/libexec/bcm-switch apply');record['after_interrupted_apply']={'rc':p.returncode,'result':p.stdout};assert p.returncode==0,p.stdout+p.stderr
        p=call('cat /tmp/bcm-sdk/native-apply.cint');assert 'native_clear_l3()' in p.stdout and 'native_clear_l2()' in p.stdout
        assert call('test ! -e /tmp/bcm-sdk/native-failed').returncode==0
        record['interrupted_apply_forced_full_rebuild']=True
        record['passed']=True
    finally:
        if owner:call('kill -9 '+str(owner))
        if child:child.wait(timeout=10)
        call('rm -f /tmp/native-lock-held; /etc/init.d/bcm-network start').check_returncode()
        data=json.dumps(record,indent=2)+'\n'
        stamp=datetime.datetime.now(datetime.timezone.utc).strftime('%Y%m%dT%H%M%SZ')
        (ROOT/'native-network'/('lock-tests-'+stamp+'.json')).write_text(data)
        (ROOT/'native-network/lock-tests-20261109.json').write_text(data)
    print('PASS: stale/empty/reused-PID marker recovery, contention, automatic unlock after SIGKILL, and interrupted SDK apply recovery')
if __name__=='__main__':main()
