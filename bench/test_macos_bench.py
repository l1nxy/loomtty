"""Run with: python3 -m unittest discover -s bench -p 'test_*.py' -v"""

import os
import platform
import pty
import threading
import time
import tty
import unittest

from terminal_workload import BULK, Terminal, distribution, payload


class WorkloadTests(unittest.TestCase):
    def test_complete_utf8_at_non_aligned_size(self):
        for name in BULK:
            with self.subTest(name=name):
                data = payload(name, 100, 30, 100003)
                self.assertGreaterEqual(len(data), 100003)
                self.assertEqual(data, payload(name, 100, 30, 100003))
                data.decode("utf-8", errors="strict")
                self.assertNotEqual(data[-1:], b"\x1b")

    def test_percentile_tail_is_not_an_average(self):
        result = distribution([1] * 98 + [20, 100])
        self.assertEqual(result["p50_ms"], 1)
        self.assertEqual(result["p99_ms"], 20)
        self.assertEqual(result["max_ms"], 100)

    def test_fence_accepts_fragmented_response(self):
        self.check_fence([b"\x1b[", b"1;", b"1R"])

    def test_fence_rejects_wrong_cursor_position(self):
        with self.assertRaisesRegex(RuntimeError, "unexpected cursor"):
            self.check_fence([b"\x1b[2;3R"])

    def test_fence_times_out_instead_of_reporting_false_completion(self):
        with self.assertRaises(TimeoutError):
            self.check_fence([b"unrelated input"])

    def check_fence(self, fragments):
        master, slave = pty.openpty()
        tty.setraw(slave)
        terminal = Terminal()
        terminal.fd = slave

        def respond():
            request = bytearray()
            while not request.endswith(b"\x1b[6n"):
                request.extend(os.read(master, 1024))
            for fragment in fragments:
                os.write(master, fragment)
                time.sleep(.005)

        thread = threading.Thread(target=respond)
        thread.start()
        try:
            terminal.fence(timeout=.2)
        finally:
            thread.join(timeout=1)
            os.close(master)
            os.close(slave)


@unittest.skipUnless(platform.system() == "Darwin", "libproc requires macOS")
class ResourceTests(unittest.TestCase):
    def test_cpu_time_agrees_with_python_process_clock(self):
        # Catches the 41.67x unit error from treating ARM Mach ticks as ns.
        from macos_bench import Process
        process = Process(os.getpid(), "self")
        before = process.read()["cpu_seconds"]
        clock = time.process_time()
        while time.process_time() - clock < .06:
            sum(i * i for i in range(1000))
        measured = process.read()["cpu_seconds"] - before
        expected = time.process_time() - clock
        self.assertAlmostEqual(measured, expected, delta=.008)


class ResultValidityTests(unittest.TestCase):
    def test_background_thread_panic_invalidates_completed_workload(self):
        from macos_bench import audit_terminal_log
        result = audit_terminal_log("loomtty", "thread 'notify-rs kqueue loop' panicked at kqueue.rs:661")
        self.assertIn("error", result)

    def test_display_change_is_not_accepted_as_same_grid(self):
        from macos_bench import audit_terminal_log
        initial = "cell: 16.0x31.0 ascent=25.0 (dpi_scale=2.00)"
        result = audit_terminal_log("loomtty", initial)
        self.assertNotIn("error", result)
        self.assertEqual(result["display"]["dpi_scale"], 2)
        result = audit_terminal_log("loomtty", initial + "\nDPI changed: scale=1.00 cell=8.0x16.0")
        self.assertIn("error", result)

    def test_config_reload_invalidates_measurement(self):
        from macos_bench import audit_terminal_log
        self.assertIn("error", audit_terminal_log("loomtty", "INFO config reloaded"))


if __name__ == "__main__":
    unittest.main()
