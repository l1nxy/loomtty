import sys,subprocess
from pathlib import Path
sys.path.insert(0,"/Users/linxy/.codex/worktrees/macos-native/loomtty/bench")
import macos_bench as b
label=sys.argv[1]
original=b.measure_phase
def profile(directory,processes,sequence,command,timeout):
    profiler=None
    if command["name"] in ("tui_full","tui_partial"):
        target=next(p for p in processes if p.label==label)
        trace=directory/(command["name"]+"-"+label+".trace")
        log=(directory/(command["name"]+"-"+label+"-xctrace.log")).open("w")
        profiler=subprocess.Popen(["xcrun","xctrace","record","--template","Time Profiler","--attach",str(target.pid),"--time-limit","20s","--output",str(trace)],stdout=log,stderr=subprocess.STDOUT)
    result=original(directory,processes,sequence,command,timeout)
    if profiler:
        try: code=profiler.wait(timeout=30)
        except subprocess.TimeoutExpired:
            profiler.terminate();profiler.wait(timeout=10);raise
        finally: log.close()
        if code:raise RuntimeError("xctrace failed; inspect recording log")
    return result
b.measure_phase=profile
sys.argv=["macos_bench.py","--apps","loomtty","--rounds","1","--mib",".25","--frames","2400","--latency-samples","20","--idle-seconds",".5","--loom-width","600","--loom-height","400","--loom","/Users/linxy/.codex/worktrees/macos-native/loomtty/target/release/loomtty","--loom-server","/Users/linxy/.codex/worktrees/macos-native/loomtty/target/release/loomtty-server","--output","/tmp/loom-pack-timeprofiler-"+label]
raise SystemExit(b.main())
