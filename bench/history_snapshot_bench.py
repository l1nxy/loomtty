#!/usr/bin/env python3
"""Isolated history snapshot diagnostic; no GUI, live daemon or vtebench timing."""
import argparse
import json
import hashlib
import platform
import selectors
from pathlib import Path
import subprocess
import time

from macos_bench import Process


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, default=Path('target/release/examples/history_snapshot_bench'))
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--rounds', type=int, default=3)
    args = parser.parse_args()
    if args.rounds < 1:
        parser.error('--rounds must be positive')
    args.output.mkdir(parents=True, exist_ok=True)
    (args.output / 'metadata.json').write_text(json.dumps({
        'platform': platform.platform(),
        'binary': str(args.binary.resolve()),
        'binary_sha256': hashlib.sha256(args.binary.read_bytes()).hexdigest(),
        'rounds': args.rounds,
        'grid': [100, 30],
        'history_rows': 10000,
        'iterations': 31,
        'timing': 'first sample cold; other 30 warm; excludes checkpoint waits and drop',
        'resources': 'physical footprint / RSS of headless diagnostic only, not GUI or daemon',
    }, indent=2) + '\n')
    for round_index in range(args.rounds):
        for case in ('plain', 'colors', 'unicode'):
            modes = ('owned', 'prepared') if round_index % 2 == 0 else ('prepared', 'owned')
            for mode in modes:
                label = f'{case}-{round_index}-{mode}'
                with (args.output / f'{label}.stderr').open('w') as errors:
                    process = subprocess.Popen([str(args.binary), mode, case], stdin=subprocess.PIPE,
                                               stdout=subprocess.PIPE, stderr=errors, text=True)
                    try:
                        probe = Process(process.pid, label)
                        samples = []
                        for stage in ('ready', 'held', 'done'):
                            with selectors.DefaultSelector() as selector:
                                selector.register(process.stdout, selectors.EVENT_READ)
                                if not selector.select(timeout=30):
                                    raise TimeoutError(f'{label}: waiting for {stage}')
                            message = json.loads(process.stdout.readline())
                            assert message['stage'] == stage, message
                            time.sleep(0.1)
                            message['resources'] = probe.read()
                            samples.append(message)
                            process.stdin.write('\n')
                            process.stdin.flush()
                        process.wait(timeout=10)
                        assert process.returncode == 0
                        (args.output / f'{label}.json').write_text(json.dumps(samples, indent=2) + '\n')
                        print(label, 'OK', flush=True)
                    finally:
                        if process.poll() is None:
                            process.terminate()
                            process.wait(timeout=10)


if __name__ == '__main__':
    main()
