"""Exported Time Profiler XML -> auditable, environment-free CPU stack summary.

Usage: python3 analyze.py input.xml client|server > summary.json
Input is xctrace's time-profile table, not its full TOC (which includes env vars).
Percentages use Running sample weights for this process; inclusive rows overlap.
"""
import collections
import json
import re
import sys
import xml.etree.ElementTree as ET


def category(frames, target):
    stack = '\n'.join(frames)
    if target == 'client':
        if 'loom_gpu::' in stack:
            return 'GPU frontend draw_frame'
        if any(s in stack for s in ('IOGPU', 'MTLCommand', 'AGX::', 'FPCAMetalLayer', 'CAMetalLayer')):
            return 'Identified Metal/driver background'
        if 'loom_render::terminal::view::update_view_from_grid' in stack:
            return 'Terminal rows/shaping'
        if 'process_server_events' in stack:
            return 'Apply server events'
        if 'update_native_state' in stack:
            return 'Native state sync'
        if 'loomtty::app::render::' in stack:
            return 'Other scene assembly'
        if 'loomtty::io::' in stack or 'loom_protocol::' in stack:
            return 'Protocol / IO'
        if len(frames) <= 3 and 'start_wqthread' in stack:
            return 'Unattributed workqueue'
        return 'Other event/runtime'
    if 'viewport_row_fingerprint' in stack:
        return 'Row fingerprint (including packing)'
    if 'write_cells_into_sm' in stack:
        return 'Cell encoding (including packing)'
    if 'process_pty_output' in stack:
        return 'PTY output processing'
    if 'process_pty_and_damage' in stack:
        return 'Other damage processing'
    return 'Other IO/tick/runtime'


def analyze(path, target):
    root = ET.parse(path).getroot()
    ids = {e.attrib['id']: e for e in root.iter() if 'id' in e.attrib}

    def resolve(e):
        return ids[e.attrib['ref']] if 'ref' in e.attrib else e

    inclusive, leaf, buckets, stacks = (collections.Counter() for _ in range(4))
    total, count = 0, 0
    processes = set()
    for row in root.iter('row'):
        columns = list(row)
        if resolve(columns[4]).text != 'Running':
            continue
        weight = int(resolve(columns[5]).text)
        frames = tuple(re.sub(r'::h[0-9a-f]{16}$', '', resolve(f).get('name', '?'))
                       for f in resolve(columns[6]) if f.tag == 'frame')
        if not frames:
            raise ValueError('Running sample without frames; refusing to silently drop it')
        processes.add(resolve(columns[2]).get('fmt'))
        total += weight
        count += 1
        stacks[frames] += weight
        leaf[frames[0]] += weight
        for frame in set(frames):
            inclusive[frame] += weight
        buckets[category(frames, target)] += weight
    if len(processes) != 1 or not total:
        raise ValueError('Expected exactly one nonempty target process')

    def ranked(counter):
        return [{'name': name, 'weight_ns': weight, 'percent': 100 * weight / total}
                for name, weight in counter.most_common()]

    return {'target': target, 'process': next(iter(processes)), 'samples': count,
            'total_running_weight_ns': total, 'buckets': ranked(buckets),
            'inclusive_overlapping': ranked(inclusive), 'leaf': ranked(leaf),
            'stacks': [{'weight_ns': weight, 'frames_leaf_first': frames}
                       for frames, weight in stacks.most_common()]}


if __name__ == '__main__':
    if len(sys.argv) != 3 or sys.argv[2] not in ('client', 'server'):
        raise SystemExit(__doc__)
    print(json.dumps(analyze(sys.argv[1], sys.argv[2]), ensure_ascii=False, indent=2))
