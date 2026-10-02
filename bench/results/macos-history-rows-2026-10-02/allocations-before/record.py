import sys,os,subprocess,json
from pathlib import Path
sys.path.insert(0,'/Users/linxy/.codex/worktrees/macos-native/loomtty/bench')
import macos_bench as b
original=b.measure_phase
os.environ['MallocStackLogging']='1'
def measured(directory,processes,sequence,command,timeout):
    result=original(directory,processes,sequence,command,timeout)
    if command.get('label') in ('warm_idle','post_idle'):
        for process in processes:
            label=command['label']+'-'+process.label
            for flags,suffix in [(['-s','--showSizes'],'sizes'),(['-s','--addresses=malloc[256k-]','--noContent'],'large')]:
                with (directory/f'{label}-{suffix}.txt').open('w') as output:
                    completed=subprocess.run(['/usr/bin/heap',*flags,str(process.pid)],stdout=output,stderr=subprocess.STDOUT,timeout=60)
                    print(label,suffix,'heap exit',completed.returncode,flush=True)
            with (directory/f'{label}-vmmap.txt').open('w') as output:
                subprocess.run(['/usr/bin/vmmap','-summary',str(process.pid)],stdout=output,stderr=subprocess.STDOUT,timeout=60)
            if command.get('label') == 'post_idle':
                with (directory/f'{label}-malloc-history.txt').open('w') as output:
                    subprocess.run(['/usr/bin/malloc_history',str(process.pid),'-allBySize','-fullStacks'],stdout=output,stderr=subprocess.STDOUT,timeout=60)
    return result
b.measure_phase=measured
raise SystemExit(b.main())
