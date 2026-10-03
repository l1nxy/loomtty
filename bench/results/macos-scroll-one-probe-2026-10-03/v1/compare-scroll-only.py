"""Targeted upstream scrolling A/B; does not replace the complete four-terminal suite."""
import sys,json,hashlib,subprocess,os,copy,datetime
from pathlib import Path
root=Path('/Users/linxy/.codex/worktrees/macos-native/loomtty')
sys.path.insert(0,str(root/'bench'))
import macos_bench as b
output=Path('/tmp/loom-scroll-ab-1003')
assert not output.exists()
output.mkdir()
meta=json.loads((root/'bench/results/macos-scroll-one-probe-2026-10-03/binaries.json').read_text())
args=b.argument_parser().parse_args(['--apps','loomtty','--rounds','3','--loom',meta['common_client'],'--loom-server',meta['baseline_server'],'--vtebench',str(root.parent/'vtebench-extras-20261003'),'--loom-width','600','--loom-height','400','--mib','.25','--output',str(output)])
class PhaseFinished(BaseException):pass
original=b.measure_phase
samples=[]

def only_scroll(directory,processes,sequence,command,timeout):
    manifest=json.loads((directory/'vtebench/manifest.json').read_text())
    benchmark=next(x for x in manifest if x['name']=='scrolling')
    command=dict(name='vtebench/scrolling',benchmark=benchmark,directory=str(directory),binary=str(args.vtebench/'target/release/vtebench'),min_bytes=16*b.MIB,max_secs=3,output_mode='original')
    phase=original(directory,processes,0,command,timeout)
    phase.update(name='vtebench/scrolling',warmup=False)
    samples.append(phase)
    raise PhaseFinished()
b.measure_phase=only_scroll
data=dict(schema_version=1,created_at=datetime.datetime.now(datetime.timezone.utc).isoformat(),binaries=meta,diagnostic_scope='targeted upstream scrolling only; no full-suite or TUI/memory safety claim',visibility='CLI auto-foreground per user instruction; not continuously monitored',runs=[])
expected=None
wake=subprocess.Popen(['caffeinate','-d','-i','-w',str(os.getpid())])
try:
    for round_index in range(3):
        order=('before','after') if round_index%2==0 else ('after','before')
        for variant in order:
            b.check_build_interference()
            samples=[]
            local=copy.copy(args)
            local.loom_server=Path(meta['baseline_server' if variant=='before' else 'candidate_server'])
            directory=output/f'r{round_index}-{variant}'
            print(round_index,variant,flush=True)
            try:
                result=b.run_one('loomtty',round_index,directory,local)
            except PhaseFinished:
                result=json.loads((directory/'result.json').read_text())
                result['phases']=samples
            result.update(variant=variant,round=round_index)
            data['runs'].append(result)
            if samples and 'error' not in result:
                p=samples[0]
                fingerprint=dict(display=result.get('display'),payload=p['payload'],output_mode=p['output_mode'])
                if expected is None:expected=fingerprint
                elif fingerprint!=expected:result['error']='grid/payload/output mode changed'
            b.save_json(output/'results.json',data)
            b.save_json(directory/'result.json',result)
            if 'error' in result:raise RuntimeError(result['error'])
            phase=samples[0]
            print('  MiB/s',phase['mib_per_second'],'CPU s',phase['resources']['cpu_seconds'],flush=True)
except BaseException as error:
    data['controller_error']=str(error)
    raise
finally:
    b.save_json(output/'results.json',data)
    wake.terminate();wake.wait()
