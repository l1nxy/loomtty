import sys
from pathlib import Path
sys.path.insert(0,"/Users/linxy/.codex/worktrees/macos-native/loomtty/bench")
import macos_bench as b
import compare_loom as c
original=b.measure_phase
def counted(directory, processes, sequence, command, timeout):
    log=directory/"terminal.log"
    def count():return log.read_text().count("bench-frame-submit")
    before=count()
    result=original(directory,processes,sequence,command,timeout)
    result["gpu_submits"]=count()-before
    return result
b.measure_phase=counted
raise SystemExit(c.main())
