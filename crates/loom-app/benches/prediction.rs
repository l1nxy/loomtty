//! Paired comparison with the former full-viewport composition loop.
//! Run: cargo bench -p loom-app --bench prediction
use loom_app::{grid::ClientPaneGrid, prediction::PredictionEngine};
use loom_config::PredictionMode;
use loom_protocol::message::PackedCell;
use std::{
    borrow::Cow,
    hint::black_box,
    time::{Duration, Instant},
};

fn legacy<'a>(engine: &PredictionEngine, grid: &'a ClientPaneGrid) -> Cow<'a, [PackedCell]> {
    if !engine.has_overlay(1) {
        return Cow::Borrowed(&grid.viewport);
    }
    let mut cells = grid.viewport.clone();
    for (i, cell) in cells.iter_mut().enumerate() {
        if let Some(replacement) = engine.get_overlay_cell(
            1,
            (i / grid.cols as usize) as u16,
            (i % grid.cols as usize) as u16,
        ) {
            *cell = replacement;
        }
    }
    Cow::Owned(cells)
}

fn measure(mut f: impl FnMut()) -> f64 {
    let start = Instant::now();
    let mut n = 0u64;
    while start.elapsed() < Duration::from_millis(250) {
        for _ in 0..100 {
            f();
        }
        n += 100;
    }
    start.elapsed().as_nanos() as f64 / n as f64
}

fn main() {
    println!("case,size,legacy_ns,row_compositor_ns,speedup");
    for (cols, rows) in [(80, 24), (240, 80)] {
        for (case, mode) in [
            ("visible", PredictionMode::Always),
            ("hidden", PredictionMode::Never),
            ("tentative", PredictionMode::Adaptive),
        ] {
            let mut grid = ClientPaneGrid::new(cols, rows, 0);
            grid.cursor_line = rows as i16 / 2;
            let mut engine = PredictionEngine::new(mode, 30, false);
            if mode == PredictionMode::Adaptive {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_micros() as u64;
                engine.on_pong(0, now - 100_000);
                assert!(engine.should_display());
            }
            engine.new_user_input_track_hidden(1, b"echo latency", &grid, 1);
            assert_eq!(
                legacy(&engine, &grid),
                engine.apply_overlay(1, Cow::Borrowed(&grid.viewport), cols, 0)
            );
            let before = measure(|| {
                black_box(legacy(black_box(&engine), black_box(&grid)));
            });
            let after = measure(|| {
                black_box(engine.apply_overlay(
                    black_box(1),
                    Cow::Borrowed(black_box(&grid.viewport)),
                    cols,
                    0,
                ));
            });
            println!(
                "{case},{cols}x{rows},{before:.0},{after:.0},{:.1}x",
                before / after
            );
        }
    }
}
