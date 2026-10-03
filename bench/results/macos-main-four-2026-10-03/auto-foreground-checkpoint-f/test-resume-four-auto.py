import ast, copy, hashlib, json, tempfile, time
from pathlib import Path
from types import SimpleNamespace
import sys
root=Path('/Users/linxy/.codex/worktrees/macos-native/loomtty')
sys.path.insert(0,str(root/'bench'))
import macos_bench as b
script=root/'target/extras-bench-20261003/resume-four-auto.py'
tree=ast.parse(script.read_text())
functions=ast.Module(body=[n for n in tree.body if isinstance(n,ast.FunctionDef)],type_ignores=[])
previous=json.loads(Path('/tmp/loom-four-auto-1003f/results.json').read_text())
prior=previous['runs'][0]
assert prior['app']=='kitty' and 'error' not in prior
for case in ('valid','binary_changed','setting_changed','incomplete_phase','upstream_changed'):
 with tempfile.TemporaryDirectory(prefix='loom-resume-unit-') as tmp:
  output=Path(tmp);current=copy.deepcopy(previous);current['settings']['output']=str(output)
  selected=copy.deepcopy(prior)
  if case=='binary_changed':current['versions']['kitty']['sha256']='invalid'
  if case=='setting_changed':current['settings']['frames']=599
  if case=='incomplete_phase':selected['phases'].pop()
  if case=='upstream_changed':current['vtebench']['commit']='invalid'
  b.save_json(output/'results.json',current)
  def no_launch(*a,**k):raise RuntimeError('Reuse unexpectedly launched a terminal')
  scope=dict(copy=copy,json=json,Path=Path,time=time,b=b,previous=previous,output=output,source=Path('/tmp/loom-four-auto-1003f'),validated=False,failed=False,completed={('kitty',0):selected},original_run=no_launch)
  exec(compile(functions,str(script),'exec'),scope)
  try:result=scope['run_one']('kitty',0,output/'r0-kitty',SimpleNamespace(**current['settings']))
  except AssertionError:
   assert case!='valid'
   print(case,'correctly rejected')
  else:
   assert case=='valid'
   assert result['source_directory'].endswith('/loom-four-auto-1003f/r0-kitty')
   assert len(result['phases'])==32
   print(case,'preserved full result and provenance without launch')
