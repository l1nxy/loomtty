#!/usr/bin/env python3
"""Private-daemon input latency under full-history resize pressure; no GUI."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shlex
import subprocess
import sys
import time

from macos_bench import Process, check_build_interference


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def run(root, server_binary, client_binary, case):
    root.mkdir(parents=True)
    runtime = root / 'tmp'
    runtime.mkdir()
    reply = root / 'reply.sock'
    # Short paths are necessary for sockaddr_un on macOS.
    assert len(str(reply).encode()) < 104
    fixture = root / 'fixture.py'
    fixture.write_text('''import os, socket, tty
from pathlib import Path
tty.setraw(0)
case = CASE
lines = []
for i in range(10030):
    text = f"line-{i:05}: abcdefghijklmnopqrstuvwxyz 0123456789"
    if case == "unicode":
        text = f"line-{i:05}: e\\u0301中 🦀 abcdefghijklmnopqrstuvwxyz"
    lines.append(text + "\\r\\n")
data = ("".join(lines) + "\\x1b]0;history-input-ready\\x07").encode()
while data:
    data = data[os.write(1, data):]
s = socket.socket(socket.AF_UNIX, socket.SOCK_DGRAM)
s.connect(REPLY)
while True:
    data = bytearray()
    while len(data) < 8:
        part = os.read(0, 8 - len(data))
        if not part:
            raise SystemExit(0)
        data.extend(part)
    s.send(data)
'''.replace('CASE', repr(case)).replace('REPLY', repr(str(reply))))
    wrapper = root / 'worker.sh'
    wrapper.write_text('#!/bin/sh\nexec ' + shlex.join([sys.executable, str(fixture)]) + '\n')
    wrapper.chmod(0o700)
    config = root / 'config.toml'
    config.write_text(f'''[terminal]
shell = {json.dumps(str(wrapper))}
scrollback_lines = 10000
[appearance]
padding = 0.0
border_width = 0.0
column_gap = 0.0
''')
    env = dict(os.environ, TMPDIR=str(runtime), XDG_RUNTIME_DIR=str(runtime),
               LOOM_CONFIG_FILE=str(config), LOOM_STATE_DIR=str(root / 'state'),
               XDG_CONFIG_HOME=str(root / 'config'), XDG_CACHE_HOME=str(root / 'cache'),
               RUST_LOG='warn')
    with (root / 'server.log').open('w') as log:
        server = subprocess.Popen([str(server_binary), '--headless'], env=env, stdout=log, stderr=log)
        try:
            sock = runtime / 'loom/loom.sock'
            deadline = time.monotonic() + 10
            while not sock.exists():
                if server.poll() is not None or time.monotonic() > deadline:
                    raise RuntimeError('private server did not start')
                time.sleep(.01)
            probe = Process(server.pid, 'server')
            before = probe.read()
            with (root / 'client.log').open('w') as errors:
                client = subprocess.Popen([str(client_binary), str(sock), str(reply)],
                                          env=env, stdout=subprocess.PIPE, stderr=errors, text=True)
                try:
                    output, _ = client.communicate(timeout=35)
                finally:
                    if client.poll() is None:
                        client.terminate()
                        client.wait(timeout=5)
            (root / 'samples.jsonl').write_text(output)
            assert client.returncode == 0, f'client failed: {root / "client.log"}'
            after = probe.read()
            phases = [json.loads(line) for line in output.splitlines()]
            assert len(phases) == 2 and all(len(p['latency_ms']) == 1000 for p in phases)
            (root / 'resources.json').write_text(json.dumps(dict(before=before, after=after), indent=2))
        finally:
            if server.poll() is None:
                server.terminate()
                server.wait(timeout=10)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--before', type=Path, required=True)
    parser.add_argument('--after', type=Path, required=True)
    parser.add_argument('--client', type=Path, default=Path('target/release/examples/history_input_latency'))
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--rounds', type=int, default=3)
    args = parser.parse_args()
    if args.rounds < 1:
        parser.error('--rounds must be positive')
    args.output = args.output.resolve()
    args.output.mkdir(parents=True, exist_ok=False)
    binaries = {k: getattr(args, k).resolve() for k in ('before', 'after', 'client')}
    (args.output / 'metadata.json').write_text(json.dumps({
        'binaries': {k: {'path': str(v), 'sha256': sha(v)} for k, v in binaries.items()},
        'rounds': args.rounds, 'samples_per_phase': 1000, 'input_interval_ms': 2,
        'resize_interval_ms': 16, 'history': 10000, 'grid': '100 columns; 29/30 rows',
        'scope': 'input socket write through daemon to PTY worker and Unix datagram reply; no renderer',
        'phase_order': ['idle', 'resize'],
    }, indent=2) + '\n')
    for index in range(args.rounds):
        for case in ('plain', 'unicode'):
            order = ('before', 'after') if index % 2 == 0 else ('after', 'before')
            for mode in order:
                check_build_interference()
                root = args.output / f'{index}-{case}-{mode}'
                run(root, binaries[mode], binaries['client'], case)
                check_build_interference()
                print(root.name, 'OK', flush=True)


if __name__ == '__main__':
    main()
