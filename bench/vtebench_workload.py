"""Run pinned, unmodified Alacritty vtebench with audited upstream payloads.

Generators run once in the real benchmark PTY before resource measurement.
Their exact bytes are replayed by executable wrappers so failures cannot be
silently skipped by upstream's filter_map, and hashes can be compared across apps.
The Rust harness, resets, warmup and write-blocking timer are unchanged.
"""
import hashlib
import math
import os
from pathlib import Path
import shlex
import statistics
import subprocess
import time

from terminal_workload import save_json


def prepare(repo, directory):
    repo, directory = Path(repo), Path(directory)
    root = directory / 'vtebench'
    root.mkdir()
    # BSD ps pads its tty column with a trailing space. Upstream embeds that
    # whole field in /dev/<tty>, which fails on macOS. Normalize only that query;
    # benchmark algorithms and the upstream Rust timer stay untouched.
    tools = root / 'tools'
    tools.mkdir()
    shim = tools / 'ps'
    shim.write_text("#!/bin/sh\nif [ \"$1\" = -o ] && [ \"$2\" = tty= ]; then\n"
                    "  /bin/ps \"$@\" | /usr/bin/awk '{$1=$1; print}'\n"
                    "else\n  exec /bin/ps \"$@\"\nfi\n")
    shim.chmod(0o700)
    manifest = []
    for source in sorted((repo / 'benchmarks').iterdir()):
        if not (source / 'benchmark').is_file():
            continue
        target = root / source.name
        target.mkdir()
        entry = {'name': source.name}
        for name in ('benchmark', 'setup'):
            script = source / name
            if not script.exists():
                continue
            process = subprocess.run([str(script)], capture_output=True, timeout=60,
                                     env={**os.environ, 'TERM': 'xterm-256color',
                                          'PATH': str(tools) + os.pathsep + os.environ['PATH']})
            if process.returncode or process.stderr:
                raise RuntimeError(f'vtebench generator {script} failed: {process.stderr.decode(errors="replace")}')
            if name == 'benchmark' and not process.stdout:
                raise RuntimeError(f'empty vtebench payload: {script}')
            payload = target / f'{name}.bytes'
            payload.write_bytes(process.stdout)
            wrapper = target / name
            wrapper.write_text('#!/bin/sh\nexec ' + shlex.join(['/bin/cat', str(payload)]) + '\n')
            wrapper.chmod(0o700)
            entry[name] = {'bytes': len(process.stdout),
                           'sha256': hashlib.sha256(process.stdout).hexdigest()}
        manifest.append(entry)
    if not manifest:
        raise RuntimeError('vtebench has no benchmarks')
    save_json(root / 'manifest.json', manifest)
    return manifest


def parse_dat(path, expected):
    lines = Path(path).read_text().splitlines()
    if not lines or lines[0].split() != [expected]:
        raise ValueError(f'vtebench did not run expected benchmark {expected}')
    samples = []
    for line in lines[1:]:
        fields = line.split()
        if len(fields) != 1 or not fields[0].isdigit():
            raise ValueError(f'invalid vtebench sample: {line!r}')
        samples.append(int(fields[0]))
    if not samples or statistics.median(samples) <= 0:
        raise ValueError('missing/zero-resolution vtebench samples; increase --min-bytes')
    return samples


def execute(terminal, command):
    directory = Path(command['directory'])
    name = command['benchmark']['name']
    output = directory / 'vtebench' / f'{name}.dat'
    error = output.with_suffix('.stderr')
    minimum = command['min_bytes']
    block = command['benchmark']['benchmark']['bytes']
    sample_bytes = math.ceil(minimum / block) * block
    argv = [command['binary'], '--benchmarks', str(directory / 'vtebench' / name),
            '--min-bytes', str(minimum), '--warmup', '1', '--max-secs', str(command['max_secs']),
            '--silent', '--dat', str(output)]
    terminal.fence()
    start = time.monotonic_ns()
    # Give the upstream harness the actual PTY, not a pipe or captured stdout.
    with terminal.output_mode(command.get('output_mode', 'original')) as output_mode:
        with error.open('wb') as stderr:
            subprocess.run(argv, stdin=terminal.fd, stdout=terminal.fd, stderr=stderr,
                           timeout=command['max_secs'] + 90, check=True)
    terminal.fence()
    elapsed = (time.monotonic_ns() - start) / 1e9
    if error.stat().st_size:
        raise RuntimeError(f'vtebench emitted errors; see {error}')
    samples = parse_dat(output, name)
    time.sleep(.25)
    return dict(name=command['name'], benchmark=name, seconds=elapsed, argv=argv,
                output_mode=output_mode,
                samples_ms=samples, sample_bytes=sample_bytes, warmup_samples=1,
                payload=command['benchmark'], median_ms=statistics.median(samples),
                mib_per_second=sample_bytes / 1048576 / (statistics.median(samples) / 1000),
                payload_bytes_written=sample_bytes * (len(samples) + 1))
