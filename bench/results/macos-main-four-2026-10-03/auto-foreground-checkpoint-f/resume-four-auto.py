"""Resume clean complete launches; preserve every interrupted attempt and provenance."""
import copy, hashlib, json, os, subprocess, sys, time
from pathlib import Path
root = Path('/Users/linxy/.codex/worktrees/macos-native/loomtty')
sys.path.insert(0, str(root / 'bench'))
import macos_bench as b
source = Path(sys.argv[1]).resolve()
output = Path(sys.argv[2]).resolve()
assert source != output and not output.exists()
previous = json.loads((source / 'results.json').read_text())
assert not previous['settings']['probe']
assert previous['settings']['rounds'] == 3
assert previous['settings']['apps'] == list(b.APPS)
assert not previous['settings']['confirm_visible']
completed = {(r['app'], r['round']): r for r in previous['runs'] if 'error' not in r}
original_run, original_report = b.run_one, b.report
validated = False
failed = False
source_batches = previous.get('source_batches', []) + [{
    'directory': str(source),
    'results_sha256': hashlib.sha256((source / 'results.json').read_bytes()).hexdigest(),
    'versions': previous['versions'],
    'benchmark_sha256': previous['benchmark_sha256'],
}]

def report(data, destination):
    data['source_batches'] = source_batches
    data['automation'] = {'script_sha256': hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
        'visibility': 'CLI auto-foreground per user; not independently or continuously verified'}
    b.save_json(destination / 'results.json', data)
    original_report(data, destination)


def run_one(app, round_index, directory, args):
    global validated, failed
    if failed:
        raise RuntimeError('Previous launch interrupted; stop before the next app, retaining completed launches')
    if not validated:
        current = json.loads((output / 'results.json').read_text())
        for key in ('versions', 'benchmark_sha256', 'vtebench'):
            assert current[key] == previous[key], f'{key} changed; cannot resume'
        for key, value in previous['settings'].items():
            if key != 'output':
                assert current['settings'][key] == value, f'setting {key} changed'
        for key in ('chip', 'macos', 'memory_bytes', 'architecture'):
            assert current['machine'][key] == previous['machine'][key], f'machine {key} changed'
        validated = True
    prior = completed.get((app, round_index))
    if prior is not None:
        result = copy.deepcopy(prior)
        manifest = result['vtebench_manifest']
        required = ['idle'] + list(b.BULK) + ['warm_idle'] + list(b.BULK) + ['latency', 'loaded_latency', 'tui_full', 'tui_partial', 'scrollback', 'post_idle']
        required += ['vtebench/' + entry['name'] for entry in manifest]
        assert len(manifest) == 12 and [p['name'] for p in result['phases']] == required
        assert (result['ready']['columns'], result['ready']['rows']) == (args.columns, args.rows)
        result.setdefault('source_batch', str(source))
        result.setdefault('source_directory', str(source / directory.name))
        directory.mkdir(parents=True)
        b.save_json(directory / 'result.json', result)
        print('  Reusing complete verified launch from ' + result['source_directory'], flush=True)
        return result
    deadline, quiet_since = time.monotonic() + 180, None
    while True:
        if time.monotonic() > deadline:
            b.save_json(output / 'controller-failure.json', {'app': app, 'round': round_index, 'error': 'No 30-second idle interval within 180 seconds'})
            raise RuntimeError('No stable idle interval within 180 seconds')
        try:
            b.check_build_interference()
            if quiet_since is None:
                quiet_since = time.monotonic()
            if time.monotonic() - quiet_since >= 30:
                break
        except RuntimeError:
            quiet_since = None
        time.sleep(2)
    result = original_run(app, round_index, directory, args)
    result.update(source_batch=str(output), source_directory=str(directory))
    result['visibility_confirmation'] = {'method': 'CLI auto-foreground per explicit user request; not independently verified by CUA', 'continuous_visibility_monitoring': False}
    if app == 'loomtty':
        result['tested_source_commit'] = 'd60c18a'
    b.save_json(directory / 'result.json', result)
    failed = 'error' in result
    return result

b.run_one, b.report = run_one, report
settings = previous['settings']
sys.argv = ['macos_bench.py']
for key, value in settings.items():
    if key == 'output':
        value = str(output)
    if isinstance(value, bool):
        if value:
            sys.argv.append('--' + key.replace('_', '-'))
    elif value is not None:
        sys.argv += ['--' + key.replace('_', '-')] + ([str(v) for v in value] if isinstance(value, list) else [str(value)])
wake = subprocess.Popen(['caffeinate', '-d', '-i', '-w', str(os.getpid())])
try:
    code = b.main()
finally:
    wake.terminate()
    wake.wait()
raise SystemExit(code)
