//! Headless daemon diagnostic driven by bench/history_input_latency.py.
//! Measures input delivery to a private PTY worker, not rendered latency.
#[cfg(unix)]
mod unix {
    use loom_protocol::{codec, message::*};
    use std::io;
    use std::sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    };
    use tokio::io::AsyncWriteExt;
    use tokio::net::{UnixDatagram, UnixStream};
    use tokio::sync::Mutex;
    use tokio::time::{Duration, Instant, timeout};

    #[tokio::main]
    pub async fn run() -> io::Result<()> {
        let args: Vec<_> = std::env::args().collect();
        assert_eq!(args.len(), 3, "socket reply-socket");
        let replies = UnixDatagram::bind(&args[2])?;
        let mut stream = UnixStream::connect(&args[1]).await?;
        codec::write_client_hello(
            &mut stream,
            &codec::ClientHello {
                session_name: "latency-test".into(),
                width: 800,
                height: 480,
                cell_width: 8.0,
                cell_height: 16.0,
            },
        )
        .await?;
        codec::read_server_hello(&mut stream).await?;
        let pane_id = loop {
            if let codec::Frame::ServerMsg(ServerMessage::StateSync { pane_ids, .. }) =
                codec::read_frame(&mut stream).await?
            {
                assert_eq!(pane_ids.len(), 1);
                break pane_ids[0];
            }
        };
        // Read until the terminal has parsed the worker's completion marker.
        // The marker is emitted after all 10,030 lines, so history is saturated.
        let mut history_rows = 0;
        let mut viewport_checked = false;
        loop {
            if let codec::Frame::FullPaneSync(sync) = codec::read_frame(&mut stream).await? {
                if sync.rows != 0 {
                    assert_eq!((sync.cols, sync.rows), (100, 30));
                    viewport_checked = true;
                }
                if sync.scrollback_replace {
                    history_rows = sync.scrollback_rows;
                } else {
                    history_rows = (history_rows + sync.scrollback_rows).min(10000);
                }
                if sync.title == "history-input-ready" {
                    assert!(viewport_checked);
                    assert_eq!(history_rows, 10000);
                    break;
                }
            }
        }
        let (mut reader, writer) = stream.into_split();
        let writer = Arc::new(Mutex::new(writer));
        let replacements = Arc::new(AtomicU64::new(0));
        let counter = replacements.clone();
        let drain = tokio::spawn(async move {
            loop {
                match codec::read_frame(&mut reader).await? {
                    codec::Frame::FullPaneSync(sync) if sync.scrollback_replace => {
                        assert_eq!(sync.cols, 100);
                        assert!((9999..=10000).contains(&sync.scrollback_rows));
                        counter.fetch_add(1, Ordering::Relaxed);
                    }
                    _ => {}
                }
            }
            #[allow(unreachable_code)]
            Ok::<(), io::Error>(())
        });
        let mut ack = [0_u8; 8];
        let mut sequence = 0_u64;
        for stress in [false, true] {
            let before = replacements.load(Ordering::Relaxed);
            let resize_writer = writer.clone();
            let resizer = stress.then(|| {
                tokio::spawn(async move {
                    let mut clock = tokio::time::interval(Duration::from_millis(16));
                    clock.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                    let mut rows = 29;
                    loop {
                        clock.tick().await;
                        let mut tx = resize_writer.lock().await;
                        codec::encode_client_msg(
                            &mut *tx,
                            &ClientMessage::Resize {
                                cols: 100,
                                rows,
                                width: 800,
                                height: rows as u32 * 16,
                                cell_width: 8.0,
                                cell_height: 16.0,
                            },
                        )
                        .await?;
                        tx.flush().await?;
                        rows = if rows == 29 { 30 } else { 29 };
                    }
                    #[allow(unreachable_code)]
                    Ok::<(), io::Error>(())
                })
            });
            let started = Instant::now();
            let mut samples = Vec::with_capacity(1000);
            let mut clock = tokio::time::interval(Duration::from_micros(2000));
            clock.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            for _ in 0..1000 {
                clock.tick().await;
                assert!(!drain.is_finished(), "frame reader failed");
                if let Some(task) = &resizer {
                    assert!(!task.is_finished(), "resizer failed");
                }
                sequence += 1;
                let start = Instant::now();
                {
                    let mut tx = writer.lock().await;
                    codec::encode_client_msg(
                        &mut *tx,
                        &ClientMessage::Input {
                            pane_id,
                            data: sequence.to_le_bytes().to_vec(),
                            input_seq: sequence,
                        },
                    )
                    .await?;
                    tx.flush().await?;
                }
                let n = timeout(Duration::from_secs(3), replies.recv(&mut ack)).await??;
                assert_eq!(n, 8);
                assert_eq!(u64::from_le_bytes(ack), sequence, "lost/reordered input");
                samples.push(start.elapsed().as_secs_f64() * 1000.0);
            }
            if let Some(task) = resizer {
                task.abort();
                let _ = task.await;
            }
            assert!(!drain.is_finished(), "frame reader failed");
            let count = replacements.load(Ordering::Relaxed) - before;
            assert!(
                !stress || count >= 50,
                "resize did not exercise full history replacement"
            );
            println!(
                "{{\"stress\":{stress},\"elapsed_s\":{},\"history_replacements\":{count},\"latency_ms\":{samples:?}}}",
                started.elapsed().as_secs_f64()
            );
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
        drain.abort();
        Ok(())
    }
}

#[cfg(unix)]
fn main() -> std::io::Result<()> {
    unix::run()
}

#[cfg(not(unix))]
fn main() {
    eprintln!("This diagnostic requires Unix sockets and a Unix PTY.");
    std::process::exit(1);
}
