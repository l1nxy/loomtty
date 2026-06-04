//! Benchmarks for protocol decode hot paths.

use loom_protocol::codec::{
    StateEncoder, decode_cell_delta_borrowed, decode_full_pane_sync, decode_sm_cells,
    encode_cell_delta_streaming_framed, encode_full_pane_sync_payload,
};
use loom_protocol::message::*;
use criterion::{Criterion, criterion_group, criterion_main};

// ─── Test data builders ─────────────────────────────────────────────

fn make_full_pane_sync(cols: u16, rows: u16, scrollback_rows: u32) -> FullPaneSync {
    let total = cols as usize * rows as usize;
    let sb_total = scrollback_rows as usize * cols as usize;

    let mut cells = Vec::with_capacity(total);
    for i in 0..total {
        if i % 3 == 0 {
            let mut c = PackedCell::with_ch('A');
            c.fg = PackedColor::rgb(200, 100, 50);
            cells.push(c);
        } else {
            cells.push(PackedCell::default());
        }
    }

    let scrollback = vec![PackedCell::default(); sb_total];

    FullPaneSync {
        meta: PaneFrameMeta {
            pane_id: 1,
            generation: 42,
            cursor_line: 10,
            cursor_col: 5,
            cursor_shape: 0,
            mode_flags: 0,
            received_ack: 100,
            echo_ack: 100,
        },
        cols,
        rows,
        title: "bench-pane".to_string(),
        scrollback,
        scrollback_rows,
        scrollback_replace: false,
        cells,
        grapheme_extras: GraphemeExtras::new(),
        hyperlink_extras: HyperlinkExtras::new(),
        cwd: Some("/home/user".to_string()),
    }
}

/// Extract raw (uncompressed) payload from an encoded frame buffer.
/// Frame format: [u8 tag][u32 LE len][payload].
/// LZ4 payload: [u32 LE uncompressed_len][lz4 data].
fn extract_payload(frame_buf: &[u8], lz4_tag: u8) -> Vec<u8> {
    let tag = frame_buf[0];
    let raw = &frame_buf[5..];
    if tag == lz4_tag {
        let ulen = u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]) as usize;
        lz4_flex::decompress(&raw[4..], ulen).unwrap()
    } else {
        raw.to_vec()
    }
}

// ─── Benchmarks ─────────────────────────────────────────────────────

fn bench_full_pane_sync_decode(c: &mut Criterion) {
    let mut group = c.benchmark_group("FullPaneSync_decode");

    for &(cols, rows, sb, label) in &[
        (80u16, 24u16, 0u32, "80x24_no_sb"),
        (160, 48, 0, "160x48_no_sb"),
        (160, 48, 1000, "160x48_1k_sb"),
    ] {
        let sync = make_full_pane_sync(cols, rows, sb);
        let payload = encode_full_pane_sync_payload(&sync).unwrap();
        group.bench_function(label, |b| {
            b.iter(|| {
                decode_full_pane_sync(criterion::black_box(&payload)).unwrap();
            });
        });
    }
    group.finish();
}

fn bench_cell_delta_decode(c: &mut Criterion) {
    let mut group = c.benchmark_group("CellDelta_decode");

    for &(cols, regions, label) in &[
        (80u16, 5usize, "80cols_5regions"),
        (160, 24, "160cols_24regions"),
    ] {
        let meta = PaneFrameMeta {
            pane_id: 1,
            generation: 42,
            cursor_line: 10,
            cursor_col: 5,
            cursor_shape: 0,
            mode_flags: 0,
            received_ack: 100,
            echo_ack: 100,
        };
        let region_list: Vec<(u16, u16, u16)> =
            (0..regions as u16).map(|i| (i, 0, cols - 1)).collect();
        let row_cells: Vec<PackedCell> = (0..cols)
            .map(|i| {
                if i % 2 == 0 {
                    PackedCell::with_ch('x')
                } else {
                    PackedCell::default()
                }
            })
            .collect();

        let mut frame_buf = Vec::new();
        encode_cell_delta_streaming_framed(
            &mut frame_buf,
            &meta,
            cols,
            &region_list,
            |_line, _l, _r, enc| {
                for cell in &row_cells {
                    enc.push_cell(cell);
                }
            },
        )
        .unwrap();

        // 0x22 = TAG_CELL_DELTA_LZ4
        let payload = extract_payload(&frame_buf, 0x22);

        group.bench_function(label, |b| {
            b.iter(|| {
                let p = criterion::black_box(payload.clone());
                decode_cell_delta_borrowed(p).unwrap();
            });
        });
    }
    group.finish();
}

fn bench_sm_decode(c: &mut Criterion) {
    let mut group = c.benchmark_group("sm_decode_cells");

    // Blank line (all default cells → OP_RESET + OP_ASCII_REPEAT)
    let blank_cells = vec![PackedCell::default(); 160];
    let mut enc = StateEncoder::new();
    for cell in &blank_cells {
        enc.push_cell(cell);
    }
    let blank_data = enc.finish().to_vec();

    // Mixed content: alternating colored + default cells
    let mut mixed_cells = Vec::with_capacity(160 * 48);
    for i in 0..160 * 48 {
        if i % 3 == 0 {
            let mut c = PackedCell::with_ch('A');
            c.fg = PackedColor::rgb(200, 100, 50);
            mixed_cells.push(c);
        } else if i % 5 == 0 {
            let mut c = PackedCell::with_ch('Z');
            c.fg = PackedColor::named(NAMED_GREEN);
            c.flags = FLAG_BOLD.to_le_bytes();
            mixed_cells.push(c);
        } else {
            mixed_cells.push(PackedCell::default());
        }
    }
    enc.reset();
    for cell in &mixed_cells {
        enc.push_cell(cell);
    }
    let mixed_data = enc.finish().to_vec();

    group.bench_function("blank_160", |b| {
        let mut out = vec![PackedCell::default(); 160];
        b.iter(|| {
            decode_sm_cells(criterion::black_box(&blank_data), &mut out).unwrap();
        });
    });

    group.bench_function("mixed_160x48", |b| {
        let mut out = vec![PackedCell::default(); 160 * 48];
        b.iter(|| {
            decode_sm_cells(criterion::black_box(&mixed_data), &mut out).unwrap();
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_full_pane_sync_decode,
    bench_cell_delta_decode,
    bench_sm_decode,
);
criterion_main!(benches);
