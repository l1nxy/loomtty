//! Storage-only diagnostic, not a terminal/rendering benchmark.
//! Build first, then run the executable without concurrent compilation.
use loom_app::grid::ScrollbackCells;
use loom_protocol::codec::{StateEncoder, decode_sm_rows, decode_sm_rows_with};
use loom_protocol::message::{PackedCell, PackedColor};
use std::hint::black_box;
use std::time::Instant;

fn measure(mut operation: impl FnMut(), iterations: usize) -> f64 {
    let start = Instant::now();
    for _ in 0..iterations {
        operation();
    }
    start.elapsed().as_secs_f64() * 1e6 / iterations as f64
}

fn main() {
    const ROWS: usize = 10_000;
    println!("columns,style,round,variant,decode_us,viewport_us");
    for columns in [100, 250] {
        for mixed in [false, true] {
            let source: Vec<_> = (0..ROWS * columns)
                .map(|i| {
                    let mut cell = PackedCell::with_ch((b'!' + (i % 90) as u8) as char);
                    if mixed {
                        cell.fg = PackedColor::indexed((i % 251) as u8);
                    }
                    cell
                })
                .collect();
            let mut encoder = StateEncoder::new();
            for cell in &source {
                encoder.push_cell(cell);
            }
            let encoded = encoder.finish();
            let dense = decode_sm_rows(encoded, columns, ROWS).unwrap();
            let compact =
                decode_sm_rows_with(encoded, columns, ROWS, |row| ScrollbackCells::from(row))
                    .unwrap();
            for (i, row) in compact.iter().enumerate() {
                assert_eq!(row.as_cells().as_ref(), dense[i]);
                assert_eq!(matches!(row, ScrollbackCells::Dense(_)), mixed);
            }
            // Each viewport copies 30 rows, just as visible_cells does when
            // scrolled. Rotate through the entire history to include cache misses.
            for round in 0..5 {
                let order = if round % 2 == 0 {
                    [false, true]
                } else {
                    [true, false]
                };
                for use_compact in order {
                    let decode_us = measure(
                        || {
                            if use_compact {
                                black_box(
                                    decode_sm_rows_with(black_box(encoded), columns, ROWS, |row| {
                                        ScrollbackCells::from(row)
                                    })
                                    .unwrap(),
                                );
                            } else {
                                black_box(
                                    decode_sm_rows(black_box(encoded), columns, ROWS).unwrap(),
                                );
                            }
                        },
                        20,
                    );
                    let mut offset = 0;
                    let viewport_us = measure(
                        || {
                            let mut cells = Vec::with_capacity(columns * 30);
                            for r in offset..offset + 30 {
                                if use_compact {
                                    cells.extend_from_slice(&compact[r].as_cells());
                                } else {
                                    cells.extend_from_slice(&dense[r]);
                                }
                            }
                            black_box(cells);
                            offset = (offset + 17) % (ROWS - 30);
                        },
                        10_000,
                    );
                    println!(
                        "{columns},{},{round},{},{decode_us:.3},{viewport_us:.3}",
                        if mixed { "mixed" } else { "uniform" },
                        if use_compact { "compact" } else { "dense" }
                    );
                }
            }
        }
    }
}
