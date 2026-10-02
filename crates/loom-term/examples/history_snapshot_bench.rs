//! Headless diagnostic for history snapshot memory and time. Controller:
//! `python3 bench/history_snapshot_bench.py --output /tmp/history-snapshot`
//! Does not connect to a running Loom server or launch a GUI.
use loom_protocol::codec::{frame_full_pane_sync, frame_prepared_pane_sync};
use loom_term::pane::Pane;
use std::hint::black_box;
use std::io::{self, Write};
use std::time::{Duration, Instant};

fn checkpoint(message: &str) {
    println!("{message}");
    io::stdout().flush().unwrap();
    let mut line = String::new();
    assert!(
        io::stdin().read_line(&mut line).unwrap() > 0,
        "controller exited"
    );
}

fn main() {
    let args: Vec<_> = std::env::args().collect();
    let mode = args.get(1).map(String::as_str).unwrap_or("prepared");
    let case = args.get(2).map(String::as_str).unwrap_or("plain");
    assert!(matches!(mode, "owned" | "prepared"));
    let text = match case {
        "plain" => "line-{i:05}: abcdefghijklmnopqrstuvwxyz 0123456789",
        "colors" => {
            "\\x1b[38;2;{i%251};{i%253};{i%255}mline-{i:05} \\x1b[48;5;{i%256}mabcdefghijk\\x1b[0m"
        }
        "unicode" => "line-{i:05}: e\\u0301中 🦀 abcdefghijklmnopqrstuvwxyz",
        _ => panic!("unknown case"),
    };
    let command = format!(
        "python3 -c 'import sys,time; sys.stdout.write(\"\".join(f\"{text}\\r\\n\" for i in range(10030)) + \"\\x1b]0;history-snapshot-ready\\x07\"); sys.stdout.flush(); time.sleep(120)'"
    );
    let mut pane = Pane::new_with_opts(1, 100, 30, "/bin/sh", Some(&command), None).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while pane.title != "history-snapshot-ready" {
        pane.process_pty_output();
        assert!(Instant::now() < deadline, "fixture timeout");
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(pane.history_size(), 10000);
    checkpoint("{\"stage\":\"ready\"}");
    let mut snapshots = Vec::with_capacity(31);
    let mut encodes = Vec::with_capacity(31);
    let mut frame_bytes = 0;
    let mut history_bytes = 0;
    for iteration in 0..31 {
        let start = Instant::now();
        if mode == "owned" {
            let sync = black_box(pane.snapshot(123));
            snapshots.push(start.elapsed().as_secs_f64() * 1000.0);
            history_bytes =
                sync.scrollback.len() * std::mem::size_of::<loom_protocol::message::PackedCell>();
            if iteration == 0 {
                checkpoint(&format!(
                    "{{\"stage\":\"held\",\"history_bytes\":{history_bytes}}}"
                ));
            }
            let start = Instant::now();
            let frame = frame_full_pane_sync(black_box(&sync)).unwrap();
            encodes.push(start.elapsed().as_secs_f64() * 1000.0);
            frame_bytes = black_box(frame).len();
        } else {
            let sync = black_box(pane.snapshot_prepared(123));
            snapshots.push(start.elapsed().as_secs_f64() * 1000.0);
            history_bytes = sync.scrollback.as_bytes().len();
            if iteration == 0 {
                checkpoint(&format!(
                    "{{\"stage\":\"held\",\"history_bytes\":{history_bytes}}}"
                ));
            }
            let start = Instant::now();
            let frame = frame_prepared_pane_sync(black_box(&sync)).unwrap();
            encodes.push(start.elapsed().as_secs_f64() * 1000.0);
            frame_bytes = black_box(frame).len();
        }
    }
    checkpoint(&format!(
        "{{\"stage\":\"done\",\"snapshot_ms\":{snapshots:?},\"encode_ms\":{encodes:?},\"frame_bytes\":{frame_bytes},\"history_bytes\":{history_bytes}}}"
    ));
}
