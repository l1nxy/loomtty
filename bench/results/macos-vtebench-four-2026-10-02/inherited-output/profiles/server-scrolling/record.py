import sys, subprocess, json
from pathlib import Path
sys.path.insert(0, '/Users/linxy/.codex/worktrees/macos-native/loomtty/bench')
import macos_bench as b
original = b.measure_phase

def measure(directory, processes, sequence, command, timeout):
    if command['name'] != 'vtebench/scrolling':
        return original(directory, processes, sequence, command, timeout)
    command = dict(command, max_secs=30)
    target = next(p for p in processes if p.label == 'server')
    trace = directory / 'scrolling-server.trace'
    with (directory / 'scrolling-server-xctrace.log').open('w') as log:
        profiler = subprocess.Popen(['xcrun', 'xctrace', 'record', '--template', 'Time Profiler',
                                     '--attach', str(target.pid), '--time-limit', '20s',
                                     '--output', str(trace)], stdout=log, stderr=subprocess.STDOUT)
        try:
            result = original(directory, processes, sequence, command, timeout)
            code = profiler.wait(timeout=30)
            if code:
                raise RuntimeError(f'xctrace exited {code}; see recording log')
            result['diagnostic_only'] = 'Time Profiler running; not an uninstrumented timing'
            return result
        finally:
            if profiler.poll() is None:
                profiler.terminate()
                profiler.wait(timeout=10)

b.measure_phase = measure
sys.argv = ['macos_bench.py', '--apps', 'loomtty', '--rounds', '1', '--frames', '600',
            '--vtebench', '/tmp/vtebench', '--vtebench-seconds', '3', '--vtebench-output-mode', 'original', '--confirm-visible',
            '--loom-width', '600', '--loom-height', '400',
            '--loom', '/tmp/Loomtty-Row-After.app/Contents/MacOS/loomtty',
            '--loom-server', '/tmp/loom-history-before/loomtty-server',
            '--output', '/tmp/loom-vtebench-scroll-inherited-profile-resume']
raise SystemExit(b.main())
