import sys,subprocess,json,time,threading,os
from pathlib import Path
root=Path('/Users/linxy/.codex/worktrees/macos-native/loomtty');sys.path.insert(0,str(root/'bench'))
import macos_bench as b
meta=json.loads((root/'bench/results/macos-delta-empty-2026-10-03/binaries.json').read_text())
output=Path('/tmp/loom-startup-probe-1003');done=threading.Event()
def inspect():
    if done.wait(10):return
    for row in subprocess.check_output(['ps','-axo','pid=,args='],text=True,errors='replace').splitlines():
        fields=row.strip().split(None,1)
        if len(fields)==2 and fields[1].startswith(meta['before_client']+' ') and 'bench-'+str(os.getpid())+'-' in fields[1]:
            subprocess.run(['sample',fields[0],'5','-file',str(output/'startup-sample.txt')],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
thread=threading.Thread(target=inspect);thread.start()
sys.argv=['macos_bench.py','--apps','loomtty','--rounds','1','--probe','--loom-width','600','--loom-height','400','--loom',meta['before_client'],'--loom-server',meta['common_server'],'--output',str(output)]
try:code=b.main()
finally:done.set();thread.join()
raise SystemExit(code)
