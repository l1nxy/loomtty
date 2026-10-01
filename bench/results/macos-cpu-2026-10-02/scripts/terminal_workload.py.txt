#!/usr/bin/env python3
"""Deterministic PTY workloads; driven by macos_bench.py inside each terminal.

DSR confirms terminal parser completion, NOT GPU presentation/input-to-photon.
Only the Python standard library is needed. No user's shell/profile is loaded.
"""

import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import re
import select
import termios
import time
import traceback
import tty

CSI = b"\x1b["
CPR = re.compile(rb"\x1b\[(\d+);(\d+)R")
BULK = ("ascii", "truecolor", "cjk", "emoji", "scroll", "cursor")


def save_json(path, value):
    path = Path(path)
    tmp = path.with_suffix(".tmp")
    tmp.write_text(json.dumps(value, ensure_ascii=False, indent=2) + "\n")
    tmp.replace(path)


def percentile(values, p):
    """Nearest-rank percentile, including the maximum at p=100."""
    ordered = sorted(values)
    return ordered[max(0, math.ceil(len(ordered) * p / 100) - 1)]


def distribution(values):
    return {"p50_ms": percentile(values, 50), "p95_ms": percentile(values, 95),
            "p99_ms": percentile(values, 99), "max_ms": max(values), "samples": len(values)}


def payload(name, columns, rows, target_bytes):
    """Repeat complete UTF-8/control-sequence blocks, never cut a sequence."""
    width = columns - 1  # Avoid implementation-dependent right-margin wrapping.
    if name == "ascii":
        block = (("0123456789 abcdefghijklmnopqrstuvwxyz " * columns)[:width] + "\r\n").encode()
    elif name == "truecolor":
        block = b"".join(f"\x1b[38;2;{i};{255-i};{i//2}mX".encode() for i in range(width))
        block += b"\x1b[0m\r\n"
    elif name == "cjk":
        chars = "终端性能测试中文字体渲染滚动窗口输入响应数据比较"
        block = ((chars * columns)[:width // 2] + "\r\n").encode()
    elif name == "emoji":
        block = (("😀🚀🌍🎉🔥💻" * columns)[:width // 2] + "\r\n").encode()
    elif name == "scroll":
        block = b"x\r\n" * 256
    elif name == "cursor":
        block = b"".join(f"\x1b[{r};1H\x1b[2K{r:03d} ".encode() + b"x" * (width - 4)
                         for r in range(1, rows + 1))
    else:
        raise ValueError(name)
    return block * max(1, math.ceil(target_bytes / len(block)))


class Terminal:
    def __enter__(self):
        self.fd = os.open("/dev/tty", os.O_RDWR | os.O_NOCTTY)
        self.original = termios.tcgetattr(self.fd)
        tty.setraw(self.fd)
        return self

    def __exit__(self, *_):
        try:
            self.write(b"\x1b[0m\x1b[?25h\x1b[?1049l")
        finally:
            termios.tcsetattr(self.fd, termios.TCSANOW, self.original)
            os.close(self.fd)

    def write(self, data):
        view = memoryview(data)
        while view:
            count = os.write(self.fd, view[:65536])
            if count <= 0:
                raise RuntimeError("PTY closed during write")
            view = view[count:]

    def fence(self, timeout=30):
        # A known cursor position makes stale/invalid replies detectable.
        self.write(b"\x1b[1;1H\x1b[6n")
        deadline = time.monotonic() + timeout
        received = bytearray()
        while time.monotonic() < deadline:
            if select.select([self.fd], [], [], max(0, deadline - time.monotonic()))[0]:
                data = os.read(self.fd, 4096)
                if not data:
                    raise RuntimeError("PTY closed while waiting for DSR")
                received.extend(data)
                match = CPR.search(received)
                if match:
                    if match.groups() != (b"1", b"1"):
                        raise RuntimeError(f"unexpected cursor response: {bytes(received)!r}")
                    return
                if len(received) > 65536:
                    raise RuntimeError("too much unrelated terminal input")
        raise TimeoutError("terminal did not acknowledge DSR; result is invalid")


def frame(index, columns, rows, partial=False):
    selected = [1 + index % rows] if partial else range(1, rows + 1)
    return b"".join(
        f"\x1b[{r};1H\x1b[38;2;{index % 256};{r * 7 % 256};180m".encode()
        + (f"{index:06d} {r:02d} Hello 世界 😀 " + "abcdef " * columns)[:columns - 12].encode()
        + b"\x1b[0m\x1b[K" for r in selected
    )


def execute(terminal, command, buffers, columns, rows):
    name = command["name"]
    result = {"name": name}
    terminal.fence()
    if name == "idle":
        start = time.monotonic_ns()
        time.sleep(command["seconds"])
    elif name in BULK:
        data = payload(name, columns, rows, 65536) if command.get("warmup") else buffers[name]
        terminal.write(b"\x1b[?1049h\x1b[2J\x1b[?25l")
        terminal.fence()
        start = time.monotonic_ns()
        terminal.write(data)
        terminal.fence()
        end = time.monotonic_ns()
        result.update(bytes=len(data), sha256=hashlib.sha256(data).hexdigest(),
                      mib_per_second=len(data) / 1048576 / ((end - start) / 1e9))
    elif name in ("latency", "loaded_latency"):
        samples = []
        terminal.write(b"\x1b[?1049h\x1b[2J\x1b[?25l")
        terminal.fence()
        data = buffers["truecolor"][:0] if name == "latency" else payload("truecolor", columns, rows, 32768)
        start = time.monotonic_ns()
        for _ in range(command["samples"]):
            tick = time.monotonic_ns()
            terminal.write(data)
            terminal.fence()
            samples.append((time.monotonic_ns() - tick) / 1e6)
            if name == "latency":
                time.sleep(.005)
        result.update(distribution(samples), roundtrips_ms=samples, bytes_per_roundtrip=len(data))
    elif name in ("tui_full", "tui_partial"):
        frames = [frame(i, columns, rows, name == "tui_partial") for i in range(command["frames"])]
        terminal.write(b"\x1b[?1049h\x1b[2J\x1b[?25l")
        terminal.fence()
        period = 1 / command["hz"]
        samples, missed = [], 0
        start = time.monotonic_ns()
        schedule = time.monotonic()
        for data in frames:
            time.sleep(max(0, schedule - time.monotonic()))
            tick = time.monotonic_ns()
            terminal.write(data)
            terminal.fence()
            samples.append((time.monotonic_ns() - tick) / 1e6)
            schedule += period
            if time.monotonic() > schedule:
                missed += 1
        time.sleep(max(0, schedule - time.monotonic()))
        result.update(distribution(samples), roundtrips_ms=samples, frames=len(frames),
                      target_hz=command["hz"], missed_deadlines=missed,
                      bytes=sum(map(len, frames)))
    elif name == "scrollback":
        # Main-screen history has different native capacity units; report separately.
        terminal.write(b"\x1b[?1049l\x1b[3J\x1b[2J\x1b[H")
        terminal.fence()
        data = b"".join((f"history {i:06d} " + "x" * (columns - 17) + "\r\n").encode()
                        for i in range(10000))
        start = time.monotonic_ns()
        terminal.write(data)
        terminal.fence()
        result.update(bytes=len(data), lines=10000)
    else:
        raise ValueError(name)
    end = locals().get("end", time.monotonic_ns())
    result["seconds"] = (end - start) / 1e9
    # Permit queued GUI/render work to settle; CPU sampling includes this tail.
    time.sleep(.25)
    return result


def worker(directory):
    directory = Path(directory)
    spec = json.loads((directory / "spec.json").read_text())
    try:
        with Terminal() as terminal:
            terminal.fence()
            ready_ns = time.monotonic_ns()
            # Allow asynchronous initial window -> PTY resize to arrive.
            time.sleep(.75)
            size = os.get_terminal_size(terminal.fd)
            save_json(directory / "ready.json", dict(pid=os.getpid(), ppid=os.getppid(),
                      ready_ns=ready_ns, columns=size.columns, rows=size.lines,
                      term=os.environ.get("TERM"), term_program=os.environ.get("TERM_PROGRAM")))
            buffers = {name: payload(name, spec["columns"], spec["rows"], spec["bytes"])
                       for name in BULK}
            save_json(directory / "prepared.json", {"ok": True})
            previous = -1
            deadline = time.monotonic() + spec["worker_timeout"]
            while time.monotonic() < deadline:
                path = directory / "command.json"
                if not path.exists():
                    time.sleep(.005)
                    continue
                command = json.loads(path.read_text())
                if command["id"] == previous:
                    time.sleep(.005)
                    continue
                previous = command["id"]
                if command["name"] == "exit":
                    return
                actual = os.get_terminal_size(terminal.fd)
                if (actual.columns, actual.lines) != (spec["columns"], spec["rows"]):
                    raise RuntimeError(f"grid changed: {actual}, expected {spec['columns']}x{spec['rows']}")
                result = execute(terminal, command, buffers, actual.columns, actual.lines)
                save_json(directory / f"phase-{previous}.json", result)
            raise TimeoutError("runner stopped communicating")
    except BaseException:
        save_json(directory / "error.json", {"error": traceback.format_exc()})
        raise


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=Path)
    worker(parser.parse_args().directory)
