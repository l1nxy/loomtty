import json,os,sys,time
from pathlib import Path
request=Path('/tmp/loom-combined-controlled-ab-1004/foreground-request.json')
d=json.loads(request.read_text())
assert d['gate_id']==sys.argv[1] and d['client_pid']==int(sys.argv[2])
os.kill(d['client_pid'],0)
Path(d['ack']).write_text(json.dumps(dict(client_pid=d['client_pid'],method='CUA AXRaise and title click; AX terminal focus verified',observed_window='loom benchmark ['+sys.argv[3]+']',time=time.time()))+'\n')
