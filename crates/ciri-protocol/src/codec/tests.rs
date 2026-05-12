use super::frame::{MAX_CONTROL_FRAME_LEN, TAG_SERVER_MSG, build_frame};
use super::handshake::{CLIENT_HELLO_HEADER_LEN, SERVER_HELLO_LEN, parse_pkg_version};
use super::state_machine::{OP_END, OP_REPEAT, OP_RESET, OP_SET_FG, sm_encode_cells};
use super::*;
use crate::message::*;
use std::io;

// ─── SM encoder/decoder roundtrip tests ─────────────────────────

#[test]
fn sm_roundtrip_blank_line() {
    // 80 default cells → should use a single Repeat opcode
    let cells: Vec<PackedCell> = vec![PackedCell::default(); 80];
    let encoded = sm_encode_cells(&cells);
    let mut decoded = vec![PackedCell::default(); 80];
    let n = decode_sm_cells(&encoded, &mut decoded).unwrap();
    assert_eq!(n, 80);
    assert_eq!(decoded, cells);
    // Should be very compact: Reset(0 if already default) + Repeat + End
    assert!(
        encoded.len() < 20,
        "blank line encoded as {}B",
        encoded.len()
    );
}

#[test]
fn sm_roundtrip_mixed_attributes() {
    let mut cells = Vec::new();
    // 3 cells with color A
    for _ in 0..3 {
        let mut c = PackedCell::with_ch('A');
        c.fg = PackedColor::rgb(255, 0, 0);
        c.bg = PackedColor::named(0);
        cells.push(c);
    }
    // 2 cells with color B + bold
    for _ in 0..2 {
        let mut c = PackedCell::with_ch('B');
        c.fg = PackedColor::named(7);
        c.bg = PackedColor::rgb(0, 0, 128);
        c.flags = FLAG_BOLD.to_le_bytes();
        cells.push(c);
    }
    // 1 cell back to default
    cells.push(PackedCell::with_ch('C'));

    let encoded = sm_encode_cells(&cells);
    let mut decoded = vec![PackedCell::default(); cells.len()];
    let n = decode_sm_cells(&encoded, &mut decoded).unwrap();
    assert_eq!(n, cells.len());
    assert_eq!(decoded, cells);
}

#[test]
fn sm_roundtrip_repeat_run() {
    // 100 identical non-default cells → Repeat
    let mut cell = PackedCell::with_ch('X');
    cell.fg = PackedColor::indexed(196);
    let cells: Vec<PackedCell> = vec![cell; 100];
    let encoded = sm_encode_cells(&cells);
    let mut decoded = vec![PackedCell::default(); 100];
    let n = decode_sm_cells(&encoded, &mut decoded).unwrap();
    assert_eq!(n, 100);
    assert_eq!(decoded, cells);
}

#[test]
fn sm_roundtrip_all_color_types() {
    let mut cells = Vec::new();
    // Named color
    let mut c = PackedCell::with_ch('N');
    c.fg = PackedColor::named(NAMED_RED);
    c.bg = PackedColor::named(NAMED_BACKGROUND);
    cells.push(c);
    // RGB color
    let mut c = PackedCell::with_ch('R');
    c.fg = PackedColor::rgb(128, 64, 32);
    c.bg = PackedColor::rgb(0, 0, 0);
    cells.push(c);
    // Indexed color
    let mut c = PackedCell::with_ch('I');
    c.fg = PackedColor::indexed(200);
    c.bg = PackedColor::indexed(50);
    cells.push(c);

    let encoded = sm_encode_cells(&cells);
    let mut decoded = vec![PackedCell::default(); 3];
    let n = decode_sm_cells(&encoded, &mut decoded).unwrap();
    assert_eq!(n, 3);
    assert_eq!(decoded, cells);
}

#[test]
fn sm_roundtrip_all_flags() {
    let flags_to_test = [
        FLAG_WIDE_CHAR,
        FLAG_BOLD,
        FLAG_ITALIC,
        FLAG_UNDERLINE,
        FLAG_INVERSE,
        FLAG_DIM,
        FLAG_STRIKEOUT,
        FLAG_HIDDEN,
        FLAG_UNDERLINE | FLAG_UNDERLINE_DOUBLE,
        FLAG_UNDERLINE | FLAG_UNDERLINE_CURLY,
    ];
    let mut cells = Vec::new();
    for (i, &f) in flags_to_test.iter().enumerate() {
        let mut c = PackedCell::with_ch(char::from(b'a' + i as u8));
        c.flags = f.to_le_bytes();
        cells.push(c);
    }
    let encoded = sm_encode_cells(&cells);
    let mut decoded = vec![PackedCell::default(); cells.len()];
    let n = decode_sm_cells(&encoded, &mut decoded).unwrap();
    assert_eq!(n, cells.len());
    assert_eq!(decoded, cells);
}

#[test]
fn sm_roundtrip_cjk_wide_char() {
    let mut cells = Vec::new();
    // Wide char cell
    let mut c = PackedCell::with_ch('中');
    c.fg = PackedColor::indexed(196);
    c.flags = FLAG_WIDE_CHAR.to_le_bytes();
    cells.push(c);
    // Spacer cell
    let s = PackedCell {
        ch_bytes: [0; 4],
        fg: PackedColor::indexed(196),
        flags: FLAG_WIDE_CHAR_SPACER.to_le_bytes(),
        ..PackedCell::default()
    };
    cells.push(s);

    let encoded = sm_encode_cells(&cells);
    let mut decoded = vec![PackedCell::default(); 2];
    let n = decode_sm_cells(&encoded, &mut decoded).unwrap();
    assert_eq!(n, 2);
    assert_eq!(decoded, cells);
}

#[test]
fn sm_compression_ratio_blank() {
    // 200x50 terminal of blank cells — should compress >99%
    let cells: Vec<PackedCell> = vec![PackedCell::default(); 200 * 50];
    let raw_size = cells.len() * PACKED_CELL_SIZE;
    let compressed = sm_encode_cells(&cells);
    assert!(
        compressed.len() < raw_size / 100,
        "SM compressed {}B vs raw {}B — ratio {:.1}%",
        compressed.len(),
        raw_size,
        compressed.len() as f64 / raw_size as f64 * 100.0
    );
}

#[test]
fn sm_compression_better_than_shell_line() {
    // Simulate 80-col shell blank line: all default cells
    let cells: Vec<PackedCell> = vec![PackedCell::default(); 80];
    let encoded = sm_encode_cells(&cells);
    // Per plan: 1120B → ~8B (99%)
    assert!(
        encoded.len() < 20,
        "shell blank line: {}B (raw 1120B)",
        encoded.len()
    );
}

#[test]
fn sm_truncated_opcode() {
    // Just a SetFg opcode with missing payload
    let data = [OP_SET_FG, 0x01]; // needs 4 more bytes
    let mut cells = [PackedCell::default(); 1];
    assert!(decode_sm_cells(&data, &mut cells).is_err());
}

#[test]
fn sm_unknown_opcode() {
    let data = [0xFE]; // unknown opcode (not 0xFF which is End)
    let mut cells = [PackedCell::default(); 1];
    assert!(decode_sm_cells(&data, &mut cells).is_err());
}

#[test]
fn sm_short_run_not_repeated() {
    // 2 identical cells followed by a different one — should NOT use Repeat (threshold is 3)
    let mut cells = vec![PackedCell::with_ch('A'); 2];
    cells.push(PackedCell::with_ch('B'));
    let encoded = sm_encode_cells(&cells);
    let mut decoded = vec![PackedCell::default(); 3];
    let n = decode_sm_cells(&encoded, &mut decoded).unwrap();
    assert_eq!(n, 3);
    assert_eq!(decoded, cells);
    // Verify no Repeat opcode was used
    assert!(!encoded.contains(&OP_REPEAT));
}

#[test]
fn sm_reset_opcode() {
    // Non-default cell followed by default cell — should use Reset opcode
    let mut cells = Vec::new();
    let mut c = PackedCell::with_ch('X');
    c.fg = PackedColor::rgb(255, 0, 0);
    c.bg = PackedColor::rgb(0, 255, 0);
    c.flags = FLAG_BOLD.to_le_bytes();
    cells.push(c);
    cells.push(PackedCell::with_ch('D')); // default attrs

    let encoded = sm_encode_cells(&cells);
    let mut decoded = vec![PackedCell::default(); 2];
    let n = decode_sm_cells(&encoded, &mut decoded).unwrap();
    assert_eq!(n, 2);
    assert_eq!(decoded, cells);
    // Verify Reset opcode was used (saves bytes vs SetFg+SetBg+SetFlags)
    assert!(encoded.contains(&OP_RESET));
}

// ─── CellDelta SM roundtrip ─────────────────────────────────────

#[test]
fn cell_delta_sm_roundtrip() {
    // Build cells for one region
    let cells = vec![
        {
            let mut c = PackedCell::with_ch('A');
            c.fg = PackedColor::named(7);
            c.bg = PackedColor::named(0);
            c
        },
        {
            let mut c = PackedCell::with_ch('B');
            c.fg = PackedColor::rgb(255, 0, 0);
            c.bg = PackedColor::named(0);
            c.flags = FLAG_BOLD.to_le_bytes();
            c
        },
        {
            let mut c = PackedCell::with_ch('C');
            c.fg = PackedColor::indexed(196);
            c.bg = PackedColor::named(0);
            c
        },
    ];

    let mut buf = Vec::new();
    let meta = PaneFrameMeta {
        pane_id: 42,
        generation: 100,
        cursor_line: 5,
        cursor_col: 10,
        cursor_shape: 0,
        mode_flags: 0,
        echo_ack: 0,
    };
    encode_cell_delta_streaming_framed(
        &mut buf,
        &meta,
        80,
        &[(5, 10, 12)],
        |_line, _left, _right, enc| {
            for c in &cells {
                enc.push_cell(c);
            }
        },
    )
    .unwrap();

    // Skip frame header (tag + len = 5 bytes) to get payload
    let payload = buf[5..].to_vec();
    let delta = decode_cell_delta_borrowed(payload).unwrap();
    assert_eq!(delta.meta.pane_id, 42);
    assert_eq!(delta.meta.generation, 100);
    assert_eq!(delta.cols, 80);
    assert_eq!(delta.regions.len(), 1);
    assert_eq!(delta.regions[0].line, 5);
    assert_eq!(delta.regions[0].left, 10);
    assert_eq!(delta.regions[0].right, 12);

    // Decode the SM data for the region
    let sm_data = delta.sm_data(0);
    let mut decoded = vec![PackedCell::default(); 3];
    let n = decode_sm_cells(sm_data, &mut decoded).unwrap();
    assert_eq!(n, 3);
    assert_eq!(decoded[0].ch(), 'A');
    assert_eq!(decoded[1].ch(), 'B');
    assert_eq!(decoded[1].flags_u16(), FLAG_BOLD);
    assert_eq!(decoded[2].ch(), 'C');
    assert_eq!(decoded[2].fg, PackedColor::indexed(196));
}

// ─── FullPaneSync SM roundtrip ──────────────────────────────────

#[test]
fn full_pane_sync_roundtrip() {
    let blank = PackedCell::default();
    let cells: Vec<PackedCell> = vec![blank; 80 * 24];
    let sync = FullPaneSync {
        meta: PaneFrameMeta {
            pane_id: 1,
            generation: 50,
            cursor_line: 0,
            cursor_col: 0,
            cursor_shape: CURSOR_BLOCK,
            mode_flags: 0,
            echo_ack: 0,
        },
        cols: 80,
        rows: 24,
        title: "bash".to_string(),
        scrollback: Vec::new(),
        scrollback_rows: 0,
        scrollback_replace: false,
        cells,
        grapheme_extras: GraphemeExtras::new(),
        hyperlink_extras: HyperlinkExtras::new(),
        cwd: None,
    };
    let payload = encode_full_pane_sync_payload(&sync).unwrap();
    // SM should compress blank cells significantly
    assert!(payload.len() < 80 * 24 * PACKED_CELL_SIZE);
    let decoded = decode_full_pane_sync(&payload).unwrap();
    assert_eq!(decoded.meta.pane_id, 1);
    assert_eq!(decoded.cols, 80);
    assert_eq!(decoded.rows, 24);
    assert_eq!(decoded.cells.len(), 80 * 24);
    assert_eq!(decoded.title, "bash");
}

#[test]
fn full_pane_sync_with_scrollback() {
    let mut sb = Vec::new();
    for i in 0..3u8 {
        for _ in 0..10 {
            let mut c = PackedCell::with_ch(char::from(b'0' + i));
            c.fg = PackedColor::indexed(i);
            sb.push(c);
        }
    }
    let cells: Vec<PackedCell> = vec![PackedCell::default(); 10 * 5];
    let sync = FullPaneSync {
        meta: PaneFrameMeta {
            pane_id: 2,
            generation: 10,
            cursor_line: 0,
            cursor_col: 0,
            cursor_shape: CURSOR_BLOCK,
            mode_flags: 0,
            echo_ack: 0,
        },
        cols: 10,
        rows: 5,
        title: "test".to_string(),
        scrollback: sb.clone(),
        scrollback_rows: 3,
        scrollback_replace: false,
        cells,
        grapheme_extras: GraphemeExtras::new(),
        hyperlink_extras: HyperlinkExtras::new(),
        cwd: None,
    };
    let payload = encode_full_pane_sync_payload(&sync).unwrap();
    let decoded = decode_full_pane_sync(&payload).unwrap();
    assert_eq!(decoded.scrollback_rows, 3);
    assert_eq!(decoded.scrollback.len(), 30);
    assert_eq!(decoded.scrollback, sb);
}

#[test]
fn full_pane_sync_rejects_truncated_mandatory_sections() {
    let sync = FullPaneSync {
        meta: PaneFrameMeta {
            pane_id: 7,
            generation: 12,
            cursor_line: 1,
            cursor_col: 2,
            cursor_shape: CURSOR_BLOCK,
            mode_flags: 0,
            echo_ack: 0,
        },
        cols: 4,
        rows: 2,
        title: "pane".to_string(),
        scrollback: vec![PackedCell::default(); 4],
        scrollback_rows: 1,
        scrollback_replace: false,
        cells: vec![PackedCell::default(); 8],
        grapheme_extras: GraphemeExtras::new(),
        hyperlink_extras: HyperlinkExtras::new(),
        cwd: None,
    };
    let payload = encode_full_pane_sync_payload(&sync).unwrap();

    let truncated_title = &payload[..27];
    assert_eq!(
        decode_full_pane_sync(truncated_title).unwrap_err().kind(),
        io::ErrorKind::InvalidData
    );

    // Truncate deep enough to land inside the viewport data section
    // (past the trailing optional extras which tolerate truncation).
    let truncated_scrollback = &payload[..payload.len() - 12];
    assert_eq!(
        decode_full_pane_sync(truncated_scrollback)
            .unwrap_err()
            .kind(),
        io::ErrorKind::InvalidData
    );
}

#[test]
fn full_pane_sync_ignores_truncated_optional_extras() {
    let mut sync = FullPaneSync {
        meta: PaneFrameMeta {
            pane_id: 9,
            generation: 99,
            cursor_line: 0,
            cursor_col: 1,
            cursor_shape: CURSOR_BLOCK,
            mode_flags: 0,
            echo_ack: 0,
        },
        cols: 2,
        rows: 1,
        title: "links".to_string(),
        scrollback: Vec::new(),
        scrollback_rows: 0,
        scrollback_replace: false,
        cells: vec![PackedCell::with_ch('A'), PackedCell::with_ch('B')],
        grapheme_extras: GraphemeExtras::new(),
        hyperlink_extras: HyperlinkExtras::new(),
        cwd: None,
    };
    sync.grapheme_extras.push(0, "é");
    sync.hyperlink_extras.cell_links.push((1, 3));
    sync.hyperlink_extras
        .link_map
        .push((3, "https://example.test".to_string()));

    let mut framed = Vec::new();
    encode_full_pane_sync_framed(&mut framed, &sync).unwrap();
    let payload = &framed[5..];

    let mut grapheme_only = sync.clone();
    grapheme_only.hyperlink_extras = HyperlinkExtras::new();
    let grapheme_only_payload = encode_full_pane_sync_payload(&grapheme_only).unwrap();
    // payload has: ...viewport | grapheme_extras | hyperlink_extras(4 bytes empty) | cwd(2 bytes)
    // Cut 7 bytes to truncate into the grapheme extras section
    let grapheme_cut = &grapheme_only_payload[..grapheme_only_payload.len() - 7];
    let decoded = decode_full_pane_sync(grapheme_cut).unwrap();
    assert_eq!(decoded.meta.pane_id, sync.meta.pane_id);
    assert_eq!(decoded.cells, sync.cells);
    assert_eq!(decoded.scrollback, sync.scrollback);
    assert!(decoded.grapheme_extras.0.is_empty());
    assert!(decoded.hyperlink_extras.cell_links.is_empty());
    assert!(decoded.hyperlink_extras.link_map.is_empty());

    let hyperlink_cut = &payload[..payload.len() - 3];
    let decoded = decode_full_pane_sync(hyperlink_cut).unwrap();
    assert_eq!(decoded.meta.pane_id, sync.meta.pane_id);
    assert_eq!(decoded.cells, sync.cells);
    assert_eq!(decoded.grapheme_extras.0.len(), 1);
    assert_eq!(decoded.hyperlink_extras.cell_links, vec![(1, 3)]);
    assert!(decoded.hyperlink_extras.link_map.is_empty());
}

#[test]
fn full_pane_sync_rejects_oversized_visible_metadata() {
    let mut sync = FullPaneSync {
        meta: PaneFrameMeta {
            pane_id: 11,
            generation: 7,
            cursor_line: 0,
            cursor_col: 0,
            cursor_shape: CURSOR_BLOCK,
            mode_flags: 0,
            echo_ack: 0,
        },
        cols: 1,
        rows: 1,
        title: "meta".to_string(),
        scrollback: Vec::new(),
        scrollback_rows: 0,
        scrollback_replace: false,
        cells: vec![PackedCell::with_ch('A')],
        grapheme_extras: GraphemeExtras::new(),
        hyperlink_extras: HyperlinkExtras::new(),
        cwd: None,
    };

    sync.grapheme_extras.push(0, &"é".repeat(128));
    let err = encode_full_pane_sync_payload(&sync).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    assert!(err.to_string().contains("grapheme extra too long"));

    sync.grapheme_extras = GraphemeExtras::new();
    sync.hyperlink_extras
        .link_map
        .push((1, "https://example.test/".repeat(4_000)));
    let err = encode_full_pane_sync_payload(&sync).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    assert!(err.to_string().contains("hyperlink URI too long"));

    sync.hyperlink_extras = HyperlinkExtras::new();
    sync.cwd = Some("/tmp/".repeat(20_000));
    let err = encode_full_pane_sync_payload(&sync).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    assert!(err.to_string().contains("cwd too long"));
}

// ─── Frame-level roundtrip ──────────────────────────────────────

#[tokio::test]
async fn frame_roundtrip_client_msg() {
    let msg = ClientMessage::Input {
        pane_id: 1,
        data: b"hello".to_vec(),
        input_seq: 0,
    };
    let mut buf = Vec::new();
    encode_client_msg(&mut buf, &msg).await.unwrap();
    let frame = read_frame(&mut &buf[..]).await.unwrap();
    match frame {
        Frame::ClientMsg(ClientMessage::Input { pane_id, data, .. }) => {
            assert_eq!(pane_id, 1);
            assert_eq!(data, b"hello");
        }
        _ => panic!("wrong frame type"),
    }
}

#[tokio::test]
async fn frame_roundtrip_cell_delta_sm() {
    let cell = PackedCell::with_ch('X');
    let mut buf = Vec::new();
    let meta = PaneFrameMeta {
        pane_id: 1,
        generation: 1,
        cursor_line: 0,
        cursor_col: 0,
        cursor_shape: 0,
        mode_flags: 0,
        echo_ack: 0,
    };
    encode_cell_delta_streaming_framed(
        &mut buf,
        &meta,
        80,
        &[(0, 0, 0)],
        |_line, _left, _right, enc| {
            enc.push_cell(&cell);
        },
    )
    .unwrap();

    let frame = read_frame(&mut &buf[..]).await.unwrap();
    match frame {
        Frame::CellDelta(d) => {
            let sm = d.sm_data(0);
            let mut decoded = [PackedCell::default(); 1];
            let n = decode_sm_cells(sm, &mut decoded).unwrap();
            assert_eq!(n, 1);
            assert_eq!(decoded[0].ch(), 'X');
        }
        _ => panic!("wrong frame type"),
    }
}

// ─── LZ4 compression roundtrip ─────────────────────────────────

#[test]
fn lz4_roundtrip_maybe_compress() {
    use super::frame::maybe_compress_payload;
    // Build a payload large enough to trigger LZ4 (>= 128 bytes).
    // Repetitive data compresses well, guaranteeing the LZ4 path is taken.
    let payload = vec![0xABu8; 512];
    let (tag, compressed) = maybe_compress_payload(0x20, 0x22, &payload);
    assert_eq!(tag, 0x22, "should use compressed tag");
    assert!(
        compressed.len() < payload.len(),
        "compressed should be smaller"
    );

    // Decompress via the same path the reader uses.
    let decompressed = super::frame::decompress_lz4_payload(&compressed).unwrap();
    assert_eq!(decompressed, payload);
}

#[test]
fn lz4_below_threshold_stays_uncompressed() {
    use super::frame::maybe_compress_payload;
    let payload = vec![0u8; 64]; // below 128-byte threshold
    let (tag, data) = maybe_compress_payload(0x20, 0x22, &payload);
    assert_eq!(tag, 0x20, "should use uncompressed tag");
    assert_eq!(data, payload);
}

#[tokio::test]
async fn frame_roundtrip_full_pane_sync_lz4() {
    // Build cells with varied content so SM encoding produces a payload
    // large enough to trigger LZ4 (>= 128 bytes).
    let cells: Vec<PackedCell> = (0..80 * 24)
        .map(|i| {
            let mut c = PackedCell::with_ch(char::from(b'A' + (i % 26) as u8));
            c.fg = PackedColor::indexed((i % 256) as u8);
            c
        })
        .collect();
    let sync = FullPaneSync {
        meta: PaneFrameMeta {
            pane_id: 42,
            generation: 7,
            cursor_line: 5,
            cursor_col: 10,
            cursor_shape: CURSOR_BLOCK,
            mode_flags: 0,
            echo_ack: 0,
        },
        cols: 80,
        rows: 24,
        title: "bash".to_string(),
        scrollback: Vec::new(),
        scrollback_rows: 0,
        scrollback_replace: false,
        cells,
        grapheme_extras: GraphemeExtras::new(),
        hyperlink_extras: HyperlinkExtras::new(),
        cwd: Some("/home/user".to_string()),
    };

    // frame_full_pane_sync encodes + LZ4 compresses
    let framed = frame_full_pane_sync(&sync).expect("frame encoding failed");
    // Verify the tag is the LZ4 variant
    assert_eq!(
        framed[0], 0x23,
        "expected TAG_FULL_PANE_SYNC_LZ4 (0x23), got 0x{:02x}",
        framed[0]
    );

    // read_frame decodes + LZ4 decompresses
    let frame = read_frame(&mut &framed[..]).await.unwrap();
    match frame {
        Frame::FullPaneSync(decoded) => {
            assert_eq!(decoded.meta.pane_id, 42);
            assert_eq!(decoded.meta.generation, 7);
            assert_eq!(decoded.cols, 80);
            assert_eq!(decoded.rows, 24);
            assert_eq!(decoded.title, "bash");
            // Verify viewport SM data can be decoded to correct cell count.
            let mut cells = vec![PackedCell::default(); 80 * 24];
            let n = decode_sm_cells(decoded.viewport_sm_data(), &mut cells).unwrap();
            assert_eq!(n, 80 * 24);
            assert_eq!(decoded.cwd.as_deref(), Some("/home/user"));
        }
        _ => panic!("expected FullPaneSync frame, got {:?}", frame),
    }
}

#[tokio::test]
async fn frame_roundtrip_cell_delta_lz4() {
    // Encode a delta with enough regions/cells to exceed the LZ4 threshold.
    let cell = PackedCell::with_ch('Z');
    let mut buf = Vec::new();
    let meta = PaneFrameMeta {
        pane_id: 99,
        generation: 5,
        cursor_line: 0,
        cursor_col: 0,
        cursor_shape: 0,
        mode_flags: 0,
        echo_ack: 0,
    };
    // 10 regions × 20 cells each — well above 128 bytes
    let regions: Vec<(u16, u16, u16)> = (0..10).map(|line| (line, 0, 19)).collect();
    encode_cell_delta_streaming_framed(
        &mut buf,
        &meta,
        80,
        &regions,
        |_line, _left, _right, enc| {
            for _ in 0..20 {
                enc.push_cell(&cell);
            }
        },
    )
    .unwrap();

    // Verify LZ4 tag was used
    assert_eq!(
        buf[0], 0x22,
        "expected TAG_CELL_DELTA_LZ4 (0x22), got 0x{:02x}",
        buf[0]
    );

    let frame = read_frame(&mut &buf[..]).await.unwrap();
    match frame {
        Frame::CellDelta(d) => {
            assert_eq!(d.meta.pane_id, 99);
            assert_eq!(d.meta.generation, 5);
            assert_eq!(d.regions.len(), 10);
            let sm = d.sm_data(0);
            let mut decoded = vec![PackedCell::default(); 20];
            let n = decode_sm_cells(sm, &mut decoded).unwrap();
            assert_eq!(n, 20);
            assert_eq!(decoded[0].ch(), 'Z');
        }
        _ => panic!("expected CellDelta frame, got {:?}", frame),
    }
}

#[test]
fn sm_repeat_exceeding_u16_max() {
    // 70000 identical cells — exceeds u16::MAX (65535), must not overflow
    let count = 70_000usize;
    let cell = PackedCell::with_ch(' ');
    let cells: Vec<PackedCell> = vec![cell; count];
    let encoded = sm_encode_cells(&cells);
    let mut decoded = vec![PackedCell::default(); count];
    let n = decode_sm_cells(&encoded, &mut decoded).unwrap();
    assert_eq!(n, count);
    assert_eq!(decoded, cells);
}

#[test]
fn decode_cell_delta_rejects_inverted_region_bounds() {
    let mut payload = Vec::new();
    payload.extend_from_slice(&1u64.to_le_bytes()); // pane_id
    payload.extend_from_slice(&2u64.to_le_bytes()); // generation
    payload.extend_from_slice(&0i16.to_le_bytes()); // cursor_line
    payload.extend_from_slice(&0u16.to_le_bytes()); // cursor_col
    payload.push(0); // cursor_shape
    payload.extend_from_slice(&0u16.to_le_bytes()); // mode_flags (u16)
    payload.extend_from_slice(&0u64.to_le_bytes()); // echo_ack
    payload.extend_from_slice(&80u16.to_le_bytes()); // cols
    payload.extend_from_slice(&1u16.to_le_bytes()); // num_regions
    payload.extend_from_slice(&3u16.to_le_bytes()); // region: line
    payload.extend_from_slice(&5u16.to_le_bytes()); // region: left (invalid: > right)
    payload.extend_from_slice(&4u16.to_le_bytes()); // region: right
    payload.extend_from_slice(&0u32.to_le_bytes()); // region: sm_data_len

    let err = decode_cell_delta_borrowed(payload).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    assert!(err.to_string().contains("left > right"));
}

#[test]
fn decode_cell_delta_rejects_truncated_region_data() {
    let mut payload = Vec::new();
    payload.extend_from_slice(&1u64.to_le_bytes()); // pane_id
    payload.extend_from_slice(&2u64.to_le_bytes()); // generation
    payload.extend_from_slice(&0i16.to_le_bytes()); // cursor_line
    payload.extend_from_slice(&0u16.to_le_bytes()); // cursor_col
    payload.push(0); // cursor_shape
    payload.extend_from_slice(&0u16.to_le_bytes()); // mode_flags (u16)
    payload.extend_from_slice(&0u64.to_le_bytes()); // echo_ack
    payload.extend_from_slice(&80u16.to_le_bytes()); // cols
    payload.extend_from_slice(&1u16.to_le_bytes()); // num_regions
    payload.extend_from_slice(&3u16.to_le_bytes()); // region: line
    payload.extend_from_slice(&4u16.to_le_bytes()); // region: left
    payload.extend_from_slice(&6u16.to_le_bytes()); // region: right
    payload.extend_from_slice(&4u32.to_le_bytes()); // region: sm_data_len (claims 4 but only 1 byte follows)
    payload.extend_from_slice(&[OP_END]);

    let err = decode_cell_delta_borrowed(payload).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    assert!(err.to_string().contains("truncated"));
}

#[tokio::test]
async fn read_client_hello_rejects_bad_magic() {
    let mut hello = build_client_hello(&ClientHello {
        session_name: "main".to_string(),
        width: 1024,
        height: 768,
        cell_width: 8.0,
        cell_height: 16.0,
    })
    .unwrap();
    hello[0..4].copy_from_slice(b"NOPE");
    let err = read_client_hello(&mut &hello[..]).await.unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    assert!(err.to_string().contains("bad magic"));
}

#[tokio::test]
async fn read_client_hello_rejects_wrong_wire_version() {
    let mut hello = build_client_hello(&ClientHello {
        session_name: "main".to_string(),
        width: 1024,
        height: 768,
        cell_width: 8.0,
        cell_height: 16.0,
    })
    .unwrap();
    hello[8] = WIRE_PROTOCOL_VERSION + 1;
    let err = read_client_hello(&mut &hello[..]).await.unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    assert!(err.to_string().contains("incompatible wire protocol"));
}

#[tokio::test]
async fn read_client_hello_rejects_invalid_utf8_session_name() {
    let mut hello = build_client_hello(&ClientHello {
        session_name: "main".to_string(),
        width: 1024,
        height: 768,
        cell_width: 8.0,
        cell_height: 16.0,
    })
    .unwrap();
    let name_start = CLIENT_HELLO_HEADER_LEN;
    hello[name_start..name_start + 4].copy_from_slice(&[0xff, 0xfe, 0xfd, 0xfc]);
    let err = read_client_hello(&mut &hello[..]).await.unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    assert!(err.to_string().contains("invalid session name utf8"));
}

#[tokio::test]
async fn read_client_hello_rejects_invalid_cell_width() {
    let mut hello = build_client_hello(&ClientHello {
        session_name: "main".to_string(),
        width: 1024,
        height: 768,
        cell_width: 8.0,
        cell_height: 16.0,
    })
    .unwrap();
    let width_offset = CLIENT_HELLO_HEADER_LEN + 4 + 4 + 4;
    hello[width_offset..width_offset + 4].copy_from_slice(&f32::NAN.to_bits().to_le_bytes());

    let err = read_client_hello(&mut &hello[..]).await.unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    assert!(err.to_string().contains("invalid cell_width"));
}

#[tokio::test]
async fn read_client_hello_rejects_zero_viewport_width() {
    let mut hello = build_client_hello(&ClientHello {
        session_name: "main".to_string(),
        width: 1024,
        height: 768,
        cell_width: 8.0,
        cell_height: 16.0,
    })
    .unwrap();
    let width_offset = CLIENT_HELLO_HEADER_LEN + 4;
    hello[width_offset..width_offset + 4].copy_from_slice(&0u32.to_le_bytes());

    let err = read_client_hello(&mut &hello[..]).await.unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    assert!(err.to_string().contains("invalid viewport width"));
}

#[tokio::test]
async fn read_server_hello_rejects_bad_magic() {
    let mut hello = [0u8; SERVER_HELLO_LEN];
    hello[0..4].copy_from_slice(b"NOPE");
    hello[4..8].copy_from_slice(&parse_pkg_version().to_le_bytes());

    let err = read_server_hello(&mut &hello[..]).await.unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    assert!(err.to_string().contains("bad magic"));
}

#[tokio::test]
async fn read_frame_rejects_unknown_tag() {
    let mut frame = vec![0x7f];
    frame.extend_from_slice(&1u32.to_le_bytes());
    frame.push(0);
    let err = read_frame(&mut &frame[..]).await.unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    assert!(err.to_string().contains("unknown frame tag"));
}

#[tokio::test]
async fn read_frame_rejects_truncated_header() {
    let frame = [TAG_SERVER_MSG, 0, 0, 0];
    let err = read_frame(&mut &frame[..]).await.unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::UnexpectedEof);
}

#[tokio::test]
async fn read_frame_rejects_truncated_payload() {
    let msg = ServerMessage::ServerShutdown;
    let payload = rmp_serde::to_vec(&msg).unwrap();
    let mut frame = build_frame(TAG_SERVER_MSG, &payload).unwrap();
    frame.pop();
    let err = read_frame(&mut &frame[..]).await.unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::UnexpectedEof);
}

#[tokio::test]
async fn read_frame_rejects_oversized_payload() {
    let mut frame = vec![TAG_SERVER_MSG];
    frame.extend_from_slice(&(MAX_CONTROL_FRAME_LEN + 1).to_le_bytes());
    let err = read_frame(&mut &frame[..]).await.unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    assert!(err.to_string().contains("frame too large"));
}

fn sample_layout_state() -> LayoutState {
    LayoutState {
        workspaces: vec![WorkspaceState {
            columns: vec![ColumnState {
                tiles: vec![
                    TileState {
                        pane_id: 10,
                        weight: 1.0,
                    },
                    TileState {
                        pane_id: 11,
                        weight: 2.0,
                    },
                ],
                active_tile_idx: 1,
                width_proportion: 0.6,
                width_fixed_px: Some(480.0),
            }],
            active_column_idx: 0,
        }],
        active_workspace_idx: 0,
    }
}

fn sample_session_info() -> SessionInfo {
    SessionInfo {
        name: "main".to_string(),
        running: true,
        pane_count: 3,
        client_count: 2,
    }
}

fn sample_session_detail_info() -> SessionDetailInfo {
    SessionDetailInfo {
        name: "main".to_string(),
        running: true,
        pane_count: 3,
        client_count: 2,
        workspace_count: 2,
        active_workspace: 1,
    }
}

fn sample_pane_detail_info() -> PaneDetailInfo {
    PaneDetailInfo {
        pane_id: 42,
        cols: 120,
        rows: 40,
        title: "shell".to_string(),
        cwd: Some("/tmp/project".to_string()),
        is_active: true,
        workspace_idx: 1,
        column_idx: 2,
        tile_idx: 0,
    }
}

fn sample_template_info() -> TemplateInfo {
    TemplateInfo {
        name: "dev".to_string(),
        description: Some("Dev workspace".to_string()),
        workspace_count: 2,
        total_panes: 5,
    }
}

#[tokio::test]
async fn frame_roundtrip_server_message_variants() {
    let layout = sample_layout_state();
    let session_info = sample_session_info();
    let session_detail = sample_session_detail_info();
    let pane_detail = sample_pane_detail_info();
    let template_info = sample_template_info();

    let cases = vec![
        ServerMessage::StateSync {
            layout: layout.clone(),
            pane_ids: vec![10, 11, 12],
        },
        ServerMessage::LayoutUpdate {
            layout: layout.clone(),
        },
        ServerMessage::PaneCreated {
            pane_id: 42,
            column_idx: 2,
            cols: 80,
            rows: 24,
        },
        ServerMessage::PaneClosed { pane_id: 42 },
        ServerMessage::ServerShutdown,
        ServerMessage::ClipboardStore {
            data: "copied text".to_string(),
        },
        ServerMessage::SessionList {
            sessions: vec![session_info.clone()],
        },
        ServerMessage::SessionSwitched {
            session_name: "main".to_string(),
        },
        ServerMessage::SessionKilled {
            session_name: "old".to_string(),
        },
        ServerMessage::Error {
            message: "boom".to_string(),
        },
        ServerMessage::Bell { pane_id: 7 },
        ServerMessage::CommandCompleted {
            pane_id: 7,
            duration_secs: 3,
            exit_code: Some(1),
        },
        ServerMessage::ImagePlacement {
            pane_id: 9,
            image_id: 5,
            col: 3,
            row: 4,
            width_cells: 6,
            height_cells: 7,
            pixel_width: 240,
            pixel_height: 112,
            display_mode: ImageDisplayMode::Cells,
            format: "png".to_string(),
            data: vec![1, 2, 3, 4],
        },
        ServerMessage::ImageDeleted { pane_id: 9 },
        ServerMessage::SessionInfoReply {
            info: session_detail.clone(),
        },
        ServerMessage::PaneListReply {
            panes: vec![pane_detail.clone()],
        },
        ServerMessage::CommandResult {
            success: true,
            message: "ok".to_string(),
            pane_id: Some(42),
        },
        ServerMessage::LayoutReply {
            layout: layout.clone(),
            session_name: "main".to_string(),
        },
        ServerMessage::TemplateApplied {
            session_name: "main".to_string(),
        },
        ServerMessage::TemplateList {
            templates: vec![template_info.clone()],
        },
        ServerMessage::TemplateSaved {
            template_name: "dev".to_string(),
        },
    ];

    for msg in cases {
        let expected = format!("{msg:?}");
        let frame = frame_server_msg(&msg).expect("server frame");
        match read_frame(&mut &frame[..]).await.unwrap() {
            Frame::ServerMsg(decoded) => assert_eq!(format!("{decoded:?}"), expected),
            other => panic!("expected server message frame, got {other:?}"),
        }
    }
}

#[tokio::test]
async fn frame_roundtrip_client_message_variants() {
    let cases = vec![
        ClientMessage::Input {
            pane_id: 42,
            data: vec![0x1b, b'[', b'A'],
            input_seq: 0,
        },
        ClientMessage::CreatePane,
        ClientMessage::SplitDown,
        ClientMessage::ClosePane { pane_id: 42 },
        ClientMessage::FocusLeft,
        ClientMessage::FocusRight,
        ClientMessage::FocusUp,
        ClientMessage::FocusDown,
        ClientMessage::MovePaneLeft,
        ClientMessage::MovePaneRight,
        ClientMessage::Resize {
            cols: 80,
            rows: 24,
            width: 1280,
            height: 720,
            cell_width: 8.0,
            cell_height: 16.0,
        },
        ClientMessage::SetColumnWidth {
            proportion: 0.6,
            fixed_px: Some(320.0),
        },
        ClientMessage::AdjustColumnSplit { delta: -0.15 },
        ClientMessage::EqualizeColumnSplit,
        ClientMessage::Attach,
        ClientMessage::Detach,
        ClientMessage::Ack { generation: 7 },
        ClientMessage::MouseInput {
            pane_id: 42,
            button: 1,
            col: 12,
            row: 6,
            pressed: true,
            modifiers: 2,
        },
        ClientMessage::SwitchWorkspace { workspace_idx: 1 },
        ClientMessage::ConsumeIntoColumn,
        ClientMessage::ExpelFromColumn,
        ClientMessage::ListSessions { all: false },
        ClientMessage::KillSession {
            session_name: "old".to_string(),
        },
        ClientMessage::KillServer,
        ClientMessage::SwitchSession {
            session_name: "main".to_string(),
        },
        ClientMessage::SetTileWeights {
            column_idx: 1,
            top_tile_idx: 0,
            top_weight: 1.5,
            bottom_weight: 2.5,
        },
        ClientMessage::AdjustColumnSplitAt {
            column_idx: 2,
            delta: 0.25,
        },
        ClientMessage::FocusChange { focused: true },
        ClientMessage::FocusPane { pane_id: 99 },
        ClientMessage::SendKeys {
            session_name: "main".to_string(),
            pane_id: 42,
            keys: vec![b'l', b's', b'\n'],
        },
        ClientMessage::SaveTemplate {
            template_name: "dev".to_string(),
            session_name: "main".to_string(),
        },
        ClientMessage::RunCommand {
            session_name: "main".to_string(),
            command: "ls".to_string(),
            cwd: Some("/tmp".to_string()),
        },
        ClientMessage::GetSessionInfo {
            session_name: "main".to_string(),
        },
        ClientMessage::ListPanes {
            session_name: "main".to_string(),
        },
        ClientMessage::FocusPaneById {
            session_name: "main".to_string(),
            pane_id: 7,
        },
        ClientMessage::ClosePaneById {
            session_name: "main".to_string(),
            pane_id: 8,
        },
        ClientMessage::CreatePaneIn {
            session_name: "main".to_string(),
        },
        ClientMessage::GetLayout {
            session_name: "main".to_string(),
        },
        ClientMessage::ApplyTemplate {
            template_name: "dev".to_string(),
            session_name: "main".to_string(),
        },
        ClientMessage::ListTemplates,
        ClientMessage::ListSessions { all: true },
    ];

    for msg in cases {
        let expected = format!("{msg:?}");
        let mut frame = Vec::new();
        encode_client_msg(&mut frame, &msg).await.unwrap();
        match read_frame(&mut &frame[..]).await.unwrap() {
            Frame::ClientMsg(decoded) => assert_eq!(format!("{decoded:?}"), expected),
            other => panic!("expected client message frame, got {other:?}"),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════
// proptest: SM codec fuzzing roundtrips
// ═══════════════════════════════════════════════════════════════════

mod proptest_roundtrips {
    use super::*;
    use proptest::prelude::*;

    fn arb_packed_color() -> impl Strategy<Value = PackedColor> {
        prop_oneof![
            (0u8..29).prop_map(PackedColor::named),
            (any::<u8>(), any::<u8>(), any::<u8>()).prop_map(|(r, g, b)| PackedColor::rgb(r, g, b)),
            any::<u8>().prop_map(PackedColor::indexed),
        ]
    }

    fn arb_cell_flags() -> impl Strategy<Value = u16> {
        prop_oneof![
            Just(0u16),
            Just(FLAG_BOLD),
            Just(FLAG_ITALIC),
            Just(FLAG_UNDERLINE),
            Just(FLAG_INVERSE),
            Just(FLAG_DIM),
            Just(FLAG_STRIKEOUT),
            Just(FLAG_HIDDEN),
            Just(FLAG_BOLD | FLAG_ITALIC),
            Just(FLAG_UNDERLINE | FLAG_UNDERLINE_DOUBLE),
            Just(FLAG_UNDERLINE | FLAG_UNDERLINE_CURLY),
            // Random combination
            (0u16..0x1FFF),
        ]
    }

    /// Generate a narrow (single-width) cell with arbitrary attributes.
    fn arb_narrow_cell() -> impl Strategy<Value = PackedCell> {
        (
            // Heavily weight ASCII (the common fast path)
            prop_oneof![
                9 => (0x20u32..0x7F).prop_map(|c| char::from_u32(c).unwrap()),
                1 => Just(' '),
            ],
            arb_packed_color(),
            arb_packed_color(),
            arb_cell_flags(),
        )
            .prop_map(|(ch, fg, bg, flags)| {
                let mut c = PackedCell::with_ch(ch);
                c.fg = fg;
                c.bg = bg;
                // Mask out WIDE_CHAR for narrow cells
                c.flags = (flags & !FLAG_WIDE_CHAR).to_le_bytes();
                c
            })
    }

    /// Generate a wide (CJK) cell followed by its spacer.
    fn arb_wide_cell_pair() -> impl Strategy<Value = (PackedCell, PackedCell)> {
        (
            prop_oneof![Just('中'), Just('日'), Just('漢'), Just('🐧')],
            arb_packed_color(),
            arb_packed_color(),
            arb_cell_flags(),
        )
            .prop_map(|(ch, fg, bg, flags)| {
                let mut c = PackedCell::with_ch(ch);
                c.fg = fg;
                c.bg = bg;
                c.flags = (flags | FLAG_WIDE_CHAR).to_le_bytes();
                let spacer = PackedCell {
                    ch_bytes: [0; 4],
                    fg,
                    bg,
                    flags: FLAG_WIDE_CHAR_SPACER.to_le_bytes(),
                    ..PackedCell::default()
                };
                (c, spacer)
            })
    }

    /// Generate a cell — mostly narrow, occasionally a wide+spacer pair flattened.
    fn arb_packed_cell() -> impl Strategy<Value = Vec<PackedCell>> {
        prop_oneof![
            8 => arb_narrow_cell().prop_map(|c| vec![c]),
            2 => arb_wide_cell_pair().prop_map(|(w, s)| vec![w, s]),
        ]
    }

    /// Flat vec of cells with correct wide-char pairing.
    fn arb_cell_vec(size: std::ops::Range<usize>) -> impl Strategy<Value = Vec<PackedCell>> {
        prop::collection::vec(arb_packed_cell(), size)
            .prop_map(|nested| nested.into_iter().flatten().collect())
    }

    proptest! {
        #![proptest_config(proptest::test_runner::Config::with_cases(100))]

        #[test]
        fn sm_codec_roundtrip_arbitrary(cells in arb_cell_vec(1..200)) {
            let encoded = sm_encode_cells(&cells);
            let mut decoded = vec![PackedCell::default(); cells.len()];
            let n = decode_sm_cells(&encoded, &mut decoded).unwrap();
            prop_assert_eq!(n, cells.len());
            prop_assert_eq!(&decoded[..n], &cells[..]);
        }

        #[test]
        fn sm_codec_roundtrip_large_grid(cells in arb_cell_vec(200..2000)) {
            let encoded = sm_encode_cells(&cells);
            let mut decoded = vec![PackedCell::default(); cells.len()];
            let n = decode_sm_cells(&encoded, &mut decoded).unwrap();
            prop_assert_eq!(n, cells.len());
            prop_assert_eq!(&decoded[..n], &cells[..]);
        }

        #[test]
        fn sm_encoded_size_bounded(cells in arb_cell_vec(1..500)) {
            let encoded = sm_encode_cells(&cells);
            let raw_size = cells.len() * PACKED_CELL_SIZE;
            // Worst case: every cell has unique attrs → ~10B opcode overhead per cell.
            // With PACKED_CELL_SIZE=16, that gives ~26B/cell vs 16B raw = ~1.6x.
            // Use 2x as a safe upper bound.
            prop_assert!(
                encoded.len() < raw_size * 2 + 16, // +16 for End/Reset overhead
                "SM encoding {}B vs raw {}B ({} cells) — exceeds 2x bound",
                encoded.len(), raw_size, cells.len()
            );
        }

        #[test]
        fn sm_uniform_cells_compress_well(
            cell in arb_narrow_cell(),
            count in 10usize..500,
        ) {
            let cells = vec![cell; count];
            let encoded = sm_encode_cells(&cells);
            // Uniform cells: one SetFg+SetBg+SetFlags + one Repeat + End ≈ constant
            prop_assert!(
                encoded.len() < 50,
                "uniform {} cells → {}B (expected <50B)",
                count, encoded.len()
            );
        }
    }

    proptest! {
        #![proptest_config(proptest::test_runner::Config::with_cases(50))]

        #[test]
        fn full_pane_sync_roundtrip_with_varied_cells(
            // Use small grids so arb_narrow_cell fills the entire grid
            cols in 4u16..40,
            rows in 2u16..15,
            title in "[a-z]{0,20}",
            cursor_line in -10i16..30,
            mode_flags in prop_oneof![Just(0u16), Just(MODE_ALT_SCREEN), Just(MODE_MOUSE_REPORT)],
        ) {
            let cell_count = cols as usize * rows as usize;
            // Generate cells that fill the grid with varied attributes per position.
            // Using deterministic generation from grid coords to ensure every cell
            // has non-default content (no padding with defaults).
            let mut cells = Vec::with_capacity(cell_count);
            for row in 0..rows {
                for col in 0..cols {
                    let mut c = PackedCell::with_ch(char::from(b'A' + (col % 26) as u8));
                    c.fg = PackedColor::rgb((row as u8).wrapping_mul(17), (col as u8).wrapping_mul(7), 42);
                    c.bg = PackedColor::named((row % 16) as u8);
                    if col % 3 == 0 { c.flags = FLAG_BOLD.to_le_bytes(); }
                    if col % 5 == 0 { c.flags = FLAG_ITALIC.to_le_bytes(); }
                    cells.push(c);
                }
            }

            let sync = FullPaneSync {
                meta: PaneFrameMeta {
                    pane_id: 42,
                    generation: 99,
                    cursor_line,
                    cursor_col: cols.saturating_sub(1),
                    cursor_shape: CURSOR_BEAM,
                    mode_flags,
                    echo_ack: 7,
                },
                cols,
                rows,
                title: title.clone(),
                scrollback: Vec::new(),
                scrollback_rows: 0,
                scrollback_replace: false,
                cells: cells.clone(),
                grapheme_extras: GraphemeExtras::new(),
                hyperlink_extras: HyperlinkExtras::new(),
                cwd: Some("/tmp".to_string()),
            };
            let payload = encode_full_pane_sync_payload(&sync).unwrap();
            let decoded = decode_full_pane_sync(&payload).unwrap();
            prop_assert_eq!(decoded.cols, cols);
            prop_assert_eq!(decoded.rows, rows);
            prop_assert_eq!(&decoded.title, &title);
            prop_assert_eq!(decoded.meta.cursor_line, cursor_line);
            prop_assert_eq!(decoded.meta.mode_flags, mode_flags);
            prop_assert_eq!(decoded.meta.echo_ack, 7);
            prop_assert_eq!(decoded.cells.len(), cell_count);
            prop_assert_eq!(&decoded.cells, &cells);
            prop_assert_eq!(decoded.cwd, Some("/tmp".to_string()));
        }
    }

    // Use a tokio runtime for async read_frame in proptest
    fn rt() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
    }

    proptest! {
        #[test]
        fn cell_delta_roundtrip_with_varied_cells(
            pane_id in 1u64..100,
            generation in 0u64..1000,
            cursor_line in -50i16..50,
            cursor_col in 0u16..200,
            echo_ack in 0u64..10000,
            // Use narrow cells only — CellDelta regions have column-based bounds
            // that must exactly match cell count.
            cells_data in prop::collection::vec(arb_narrow_cell(), 1..20),
        ) {
            let right = cells_data.len().saturating_sub(1) as u16;
            let line = cursor_line.unsigned_abs();
            let meta = PaneFrameMeta {
                pane_id,
                generation,
                cursor_line,
                cursor_col,
                cursor_shape: CURSOR_BLOCK,
                mode_flags: 0,
                echo_ack,
            };
            let mut buf = Vec::new();
            encode_cell_delta_streaming_framed(
                &mut buf, &meta, 80, &[(line, 0, right)],
                |_line, _left, _right, enc| {
                    for c in &cells_data { enc.push_cell(c); }
                },
            ).unwrap();
            // Use read_frame to properly handle LZ4 decompression
            let delta = rt().block_on(async {
                match read_frame(&mut &buf[..]).await.unwrap() {
                    Frame::CellDelta(d) => d,
                    other => panic!("expected CellDelta, got {other:?}"),
                }
            });
            prop_assert_eq!(delta.meta.pane_id, pane_id);
            prop_assert_eq!(delta.meta.generation, generation);
            prop_assert_eq!(delta.meta.cursor_line, cursor_line);
            prop_assert_eq!(delta.meta.cursor_col, cursor_col);
            prop_assert_eq!(delta.meta.echo_ack, echo_ack);
            // Decode SM data and verify cell content
            let sm_data = delta.sm_data(0);
            let mut decoded = vec![PackedCell::default(); cells_data.len()];
            let n = decode_sm_cells(sm_data, &mut decoded).unwrap();
            prop_assert_eq!(n, cells_data.len());
            prop_assert_eq!(&decoded[..n], &cells_data[..]);
        }
    }
}

// ═══════════════════════════════════════════════════════════════════
// Protocol network edge cases
// ═══════════════════════════════════════════════════════════════════

mod network_edge_cases {
    use super::*;

    // ─── Handshake version mismatch ─────────────────────────────

    #[tokio::test]
    async fn handshake_rejects_wrong_magic() {
        let mut buf = Vec::new();
        buf.extend_from_slice(b"NOPE"); // wrong magic
        buf.extend_from_slice(&parse_pkg_version().to_le_bytes());
        buf.push(super::super::handshake::WIRE_PROTOCOL_VERSION);
        buf.extend_from_slice(&5u16.to_le_bytes()); // session name len
        buf.extend_from_slice(b"hello");
        buf.extend_from_slice(&1024u32.to_le_bytes()); // width
        buf.extend_from_slice(&768u32.to_le_bytes()); // height
        buf.extend_from_slice(&8.0f32.to_bits().to_le_bytes());
        buf.extend_from_slice(&16.0f32.to_bits().to_le_bytes());

        let err = read_client_hello(&mut &buf[..]).await.unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        assert!(err.to_string().contains("bad magic"));
    }

    #[tokio::test]
    async fn handshake_rejects_wrong_wire_protocol_version() {
        let mut buf = Vec::new();
        buf.extend_from_slice(b"CIRI");
        buf.extend_from_slice(&parse_pkg_version().to_le_bytes());
        buf.push(WIRE_PROTOCOL_VERSION + 1); // wrong wire version
        buf.extend_from_slice(&4u16.to_le_bytes());
        buf.extend_from_slice(b"test");
        buf.extend_from_slice(&1024u32.to_le_bytes());
        buf.extend_from_slice(&768u32.to_le_bytes());
        buf.extend_from_slice(&8.0f32.to_bits().to_le_bytes());
        buf.extend_from_slice(&16.0f32.to_bits().to_le_bytes());

        let err = read_client_hello(&mut &buf[..]).await.unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        assert!(err.to_string().contains("incompatible wire protocol"));
    }

    #[tokio::test]
    async fn handshake_rejects_invalid_viewport_dims() {
        let hello = ClientHello {
            session_name: "test".to_string(),
            width: 0, // invalid
            height: 768,
            cell_width: 8.0,
            cell_height: 16.0,
        };
        let buf = build_client_hello(&hello).unwrap();
        let err = read_client_hello(&mut &buf[..]).await.unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        assert!(err.to_string().contains("invalid viewport width"));
    }

    #[tokio::test]
    async fn handshake_rejects_extreme_cell_size() {
        let hello = ClientHello {
            session_name: "test".to_string(),
            width: 1024,
            height: 768,
            cell_width: 999.0, // too big (limit 200)
            cell_height: 16.0,
        };
        let buf = build_client_hello(&hello).unwrap();
        let err = read_client_hello(&mut &buf[..]).await.unwrap_err();
        assert!(err.to_string().contains("invalid cell_width"));
    }

    #[tokio::test]
    async fn handshake_rejects_oversized_session_name() {
        let hello = ClientHello {
            session_name: "a".repeat(256),
            width: 1024,
            height: 768,
            cell_width: 8.0,
            cell_height: 16.0,
        };
        let err = build_client_hello(&hello).unwrap_err();
        assert!(err.to_string().contains("session name too long"));
    }

    #[tokio::test]
    async fn server_hello_roundtrip() {
        let mut buf = Vec::new();
        write_server_hello(&mut buf).await.unwrap();
        assert_eq!(buf.len(), SERVER_HELLO_LEN);
        let compat = read_server_hello(&mut &buf[..]).await.unwrap();
        assert!(matches!(compat, VersionCompat::Exact(_)));
    }

    #[tokio::test]
    async fn server_hello_minor_mismatch_returns_minor_mismatch() {
        // Build a ServerHello with bumped minor version
        let local = parse_pkg_version();
        let local_minor = (local >> 16) & 0xFF;
        let bumped = (local & 0xFF00FFFF) | ((local_minor + 1) << 16);

        let mut buf = [0u8; SERVER_HELLO_LEN];
        buf[0..4].copy_from_slice(b"CIRI");
        buf[4..8].copy_from_slice(&bumped.to_le_bytes());

        let compat = read_server_hello(&mut &buf[..]).await.unwrap();
        assert!(
            matches!(compat, VersionCompat::MinorMismatch { .. }),
            "expected MinorMismatch, got {compat:?}"
        );
    }

    #[tokio::test]
    async fn server_hello_patch_mismatch_returns_patch_mismatch() {
        // Build a ServerHello with bumped patch version
        let local = parse_pkg_version();
        let local_patch = local & 0xFFFF;
        let bumped = (local & 0xFFFF0000) | ((local_patch + 1) & 0xFFFF);

        let mut buf = [0u8; SERVER_HELLO_LEN];
        buf[0..4].copy_from_slice(b"CIRI");
        buf[4..8].copy_from_slice(&bumped.to_le_bytes());

        let compat = read_server_hello(&mut &buf[..]).await.unwrap();
        assert!(
            matches!(compat, VersionCompat::PatchMismatch { .. }),
            "expected PatchMismatch, got {compat:?}"
        );
    }

    #[tokio::test]
    async fn server_hello_major_mismatch_is_rejected() {
        let local = parse_pkg_version();
        let local_major = (local >> 24) & 0xFF;
        let bumped = ((local_major + 1) << 24) | (local & 0x00FFFFFF);

        let mut buf = [0u8; SERVER_HELLO_LEN];
        buf[0..4].copy_from_slice(b"CIRI");
        buf[4..8].copy_from_slice(&bumped.to_le_bytes());

        let err = read_server_hello(&mut &buf[..]).await.unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        assert!(err.to_string().contains("incompatible major version"));
    }

    // ─── Frame size limits ──────────────────────────────────────

    #[tokio::test]
    async fn frame_rejects_oversized_control_message() {
        // Build a frame with TAG_SERVER_MSG but payload claiming > 1 MiB
        let mut buf = Vec::new();
        buf.push(TAG_SERVER_MSG);
        buf.extend_from_slice(&(MAX_CONTROL_FRAME_LEN + 1).to_le_bytes());
        // No actual payload needed — the header check should reject it

        let err = read_frame(&mut &buf[..]).await.unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        assert!(err.to_string().contains("frame too large"));
    }

    #[tokio::test]
    async fn frame_rejects_unknown_tag() {
        let frame = build_frame(0xFE, b"garbage").unwrap();
        let err = read_frame(&mut &frame[..]).await.unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        assert!(err.to_string().contains("unknown frame tag"));
    }

    #[tokio::test]
    async fn frame_rejects_truncated_header() {
        // Only 3 bytes — need 5 for header
        let buf = [0x01, 0x00, 0x00];
        let err = read_frame(&mut &buf[..]).await.unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::UnexpectedEof);
    }

    #[tokio::test]
    async fn frame_rejects_truncated_payload() {
        // Header says 100 bytes, but only 10 available
        let mut buf = Vec::new();
        buf.push(TAG_SERVER_MSG);
        buf.extend_from_slice(&100u32.to_le_bytes());
        buf.extend_from_slice(&[0u8; 10]); // only 10 bytes

        let err = read_frame(&mut &buf[..]).await.unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::UnexpectedEof);
    }

    #[tokio::test]
    async fn frame_rejects_corrupted_msgpack_payload() {
        let frame = build_frame(TAG_SERVER_MSG, b"not valid msgpack").unwrap();
        let err = read_frame(&mut &frame[..]).await.unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    }

    // ─── Partial / split reads via tokio duplex ─────────────────

    #[tokio::test]
    async fn codec_handles_split_reads_via_scripted_io() {
        // Use tokio_test::io::Builder for deterministic byte-level split control
        let msg1 = ClientMessage::Ping {
            seq: 1,
            client_time_us: 100,
        };
        let msg2 = ClientMessage::Ping {
            seq: 2,
            client_time_us: 200,
        };

        let mut frame1_bytes = Vec::new();
        encode_client_msg(&mut frame1_bytes, &msg1).await.unwrap();
        let frame1_len = frame1_bytes.len();

        let mut wire = frame1_bytes;
        encode_client_msg(&mut wire, &msg2).await.unwrap();

        // Split points: mid-header of frame 1 (3 of 5 bytes), mid-payload of frame 1,
        // then exact frame boundary, then frame 2 in one piece.
        let mid_payload = frame1_len / 2 + 3; // well inside frame 1's payload
        let mut reader = tokio_test::io::Builder::new()
            .read(&wire[..3]) // partial header
            .read(&wire[3..mid_payload]) // rest of header + partial payload
            .read(&wire[mid_payload..frame1_len]) // rest of frame 1
            .read(&wire[frame1_len..]) // all of frame 2
            .build();

        let frame1 = read_frame(&mut reader).await.unwrap();
        let frame2 = read_frame(&mut reader).await.unwrap();

        match frame1 {
            Frame::ClientMsg(ClientMessage::Ping { seq: 1, .. }) => {}
            other => panic!("expected Ping seq=1, got {other:?}"),
        }
        match frame2 {
            Frame::ClientMsg(ClientMessage::Ping { seq: 2, .. }) => {}
            other => panic!("expected Ping seq=2, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn codec_handles_large_full_pane_sync_split_reads() {
        // Build a FullPaneSync with enough data to exercise LZ4 + fragmented transport
        let mut cells = Vec::new();
        for i in 0u8..120 {
            for j in 0u8..40 {
                let mut c = PackedCell::with_ch(char::from(b'A' + (i % 26)));
                c.fg = PackedColor::rgb(i, j, 0);
                cells.push(c);
            }
        }
        let sync = FullPaneSync {
            meta: PaneFrameMeta {
                pane_id: 99,
                generation: 55,
                cursor_line: 10,
                cursor_col: 20,
                cursor_shape: CURSOR_BLOCK,
                mode_flags: MODE_ALT_SCREEN,
                echo_ack: 42,
            },
            cols: 120,
            rows: 40,
            title: "large-sync".to_string(),
            scrollback: Vec::new(),
            scrollback_rows: 0,
            scrollback_replace: false,
            cells,
            grapheme_extras: GraphemeExtras::new(),
            hyperlink_extras: HyperlinkExtras::new(),
            cwd: Some("/home/test".to_string()),
        };
        let framed = frame_full_pane_sync(&sync).unwrap();
        assert!(
            framed.len() > 200,
            "frame should be large enough: {}B",
            framed.len()
        );

        // Split into many small chunks (simulate tiny MTU / slow transport)
        let chunk_size = 32;
        let mut builder = tokio_test::io::Builder::new();
        for chunk in framed.chunks(chunk_size) {
            builder.read(chunk);
        }
        let mut reader = builder.build();

        match read_frame(&mut reader).await.unwrap() {
            Frame::FullPaneSync(decoded) => {
                assert_eq!(decoded.meta.pane_id, 99);
                assert_eq!(decoded.meta.generation, 55);
                assert_eq!(decoded.meta.mode_flags, MODE_ALT_SCREEN);
                assert_eq!(decoded.cols, 120);
                assert_eq!(decoded.rows, 40);
            }
            other => panic!("expected FullPaneSync, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn codec_detects_eof_via_scripted_io() {
        // Deliver exactly 3 bytes of a 5-byte header, then EOF
        let mut reader = tokio_test::io::Builder::new()
            .read(&[0x01, 0x00, 0x00])
            .build();

        let err = read_frame(&mut reader).await.unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::UnexpectedEof);
    }

    #[tokio::test]
    async fn handshake_over_duplex_roundtrip() {
        let (client_r, server_w) = tokio::io::duplex(256);
        let (server_r, client_w) = tokio::io::duplex(256);

        let client_hello = ClientHello {
            session_name: "integration-test".to_string(),
            width: 1920,
            height: 1080,
            cell_width: 9.5,
            cell_height: 18.0,
        };

        let hello_clone = client_hello.clone();
        let client_task = tokio::spawn(async move {
            let mut w = tokio::io::BufWriter::new(client_w);
            let mut r = tokio::io::BufReader::new(client_r);
            write_client_hello(&mut w, &hello_clone).await.unwrap();
            use tokio::io::AsyncWriteExt;
            w.flush().await.unwrap();
            read_server_hello(&mut r).await.unwrap()
        });

        let server_task = tokio::spawn(async move {
            let mut r = tokio::io::BufReader::new(server_r);
            let mut w = tokio::io::BufWriter::new(server_w);
            let (compat, hello) = read_client_hello(&mut r).await.unwrap();
            write_server_hello(&mut w).await.unwrap();
            use tokio::io::AsyncWriteExt;
            w.flush().await.unwrap();
            (compat, hello)
        });

        let client_compat = client_task.await.unwrap();
        let (server_compat, received_hello) = server_task.await.unwrap();

        assert!(matches!(client_compat, VersionCompat::Exact(_)));
        assert!(matches!(server_compat, VersionCompat::Exact(_)));
        assert_eq!(received_hello.session_name, "integration-test");
        assert_eq!(received_hello.width, 1920);
        assert_eq!(received_hello.height, 1080);
        assert!((received_hello.cell_width - 9.5).abs() < 0.01);
        assert!((received_hello.cell_height - 18.0).abs() < 0.01);
    }

    // ─── LZ4 compression edge cases ────────────────────────────

    #[test]
    fn full_pane_sync_frame_applies_lz4_for_large_payload() {
        let cells = vec![PackedCell::default(); 80 * 24];
        let sync = FullPaneSync {
            meta: PaneFrameMeta {
                pane_id: 1,
                generation: 1,
                cursor_line: 0,
                cursor_col: 0,
                cursor_shape: CURSOR_BLOCK,
                mode_flags: 0,
                echo_ack: 0,
            },
            cols: 80,
            rows: 24,
            title: "test".to_string(),
            scrollback: Vec::new(),
            scrollback_rows: 0,
            scrollback_replace: false,
            cells,
            grapheme_extras: GraphemeExtras::new(),
            hyperlink_extras: HyperlinkExtras::new(),
            cwd: None,
        };
        let framed = frame_full_pane_sync(&sync).unwrap();
        // Verify it roundtrips through read_frame (which handles decompression)
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            match read_frame(&mut &framed[..]).await.unwrap() {
                Frame::FullPaneSync(borrowed) => {
                    assert_eq!(borrowed.meta.pane_id, 1);
                    assert_eq!(borrowed.cols, 80);
                    assert_eq!(borrowed.rows, 24);
                }
                other => panic!("expected FullPaneSync, got {other:?}"),
            }
        });
    }

    // ─── Multiple frames in sequence ────────────────────────────

    #[tokio::test]
    async fn multiple_frame_types_in_sequence() {
        let mut wire = Vec::new();

        // 1. ServerMessage
        let msg = ServerMessage::Bell { pane_id: 1 };
        let frame1 = frame_server_msg(&msg).unwrap();
        wire.extend_from_slice(&frame1);

        // 2. FullPaneSync
        let sync = FullPaneSync {
            meta: PaneFrameMeta {
                pane_id: 1,
                generation: 1,
                cursor_line: 0,
                cursor_col: 0,
                cursor_shape: CURSOR_BLOCK,
                mode_flags: 0,
                echo_ack: 0,
            },
            cols: 4,
            rows: 2,
            title: "t".to_string(),
            scrollback: Vec::new(),
            scrollback_rows: 0,
            scrollback_replace: false,
            cells: vec![PackedCell::default(); 8],
            grapheme_extras: GraphemeExtras::new(),
            hyperlink_extras: HyperlinkExtras::new(),
            cwd: None,
        };
        let frame2 = frame_full_pane_sync(&sync).unwrap();
        wire.extend_from_slice(&frame2);

        // 3. Another ServerMessage
        let msg2 = ServerMessage::PaneClosed { pane_id: 1 };
        let frame3 = frame_server_msg(&msg2).unwrap();
        wire.extend_from_slice(&frame3);

        // Read all three back
        let mut cursor = &wire[..];
        match read_frame(&mut cursor).await.unwrap() {
            Frame::ServerMsg(ServerMessage::Bell { pane_id: 1 }) => {}
            other => panic!("frame 1: expected Bell, got {other:?}"),
        }
        match read_frame(&mut cursor).await.unwrap() {
            Frame::FullPaneSync(borrowed) => {
                assert_eq!(borrowed.meta.pane_id, 1);
                assert_eq!(borrowed.cols, 4);
            }
            other => panic!("frame 2: expected FullPaneSync, got {other:?}"),
        }
        match read_frame(&mut cursor).await.unwrap() {
            Frame::ServerMsg(ServerMessage::PaneClosed { pane_id: 1 }) => {}
            other => panic!("frame 3: expected PaneClosed, got {other:?}"),
        }
    }

    /// rmp-serde encodes a struct variant as a 1-element map keyed on the
    /// variant **name** (not its position in the enum), with the inner
    /// struct serialised as a positional array. Variant order in the
    /// source enum therefore does NOT affect wire-compat, but:
    ///   - renaming the variant breaks every old peer
    ///   - reordering or removing struct fields shifts the array and
    ///     breaks every old peer
    /// This test pins both the name bytes and the payload shape.
    #[test]
    fn client_message_capture_pane_wire_format_pinned() {
        let msg = ClientMessage::CapturePane {
            session_name: "s".into(),
            pane_id: 1,
            opts: Default::default(),
        };
        let bytes = rmp_serde::to_vec(&msg).unwrap();
        // Expected layout:
        //   0x81                  fixmap of size 1
        //   0xAB                  fixstr length 11 ("CapturePane")
        //   "CapturePane"         11 bytes of UTF-8
        //   0x93                  fixarray of size 3 (struct payload)
        //   ... session_name, pane_id, opts ...
        assert_eq!(bytes[0], 0x81, "expected fixmap-of-size-1 prefix");
        assert_eq!(bytes[1], 0xAB, "expected fixstr length 11 (CapturePane)");
        assert_eq!(
            &bytes[2..13],
            b"CapturePane",
            "variant NAME bytes must stay 'CapturePane' on the wire — \
             renaming breaks every old peer"
        );
        assert_eq!(
            bytes[13], 0x93,
            "struct payload must be fixarray of size 3 — \
             adding/reordering/removing fields breaks every old peer"
        );

        let decoded: ClientMessage = rmp_serde::from_slice(&bytes).unwrap();
        assert!(matches!(decoded, ClientMessage::CapturePane { .. }));
    }

    #[test]
    fn server_message_pane_capture_wire_format_pinned() {
        let msg = ServerMessage::PaneCapture {
            session_name: "s".into(),
            pane_id: 1,
            text: "hi".into(),
            truncated: false,
        };
        let bytes = rmp_serde::to_vec(&msg).unwrap();
        // Expected layout:
        //   0x81                  fixmap of size 1
        //   0xAB                  fixstr length 11 ("PaneCapture")
        //   "PaneCapture"         11 bytes of UTF-8
        //   0x94                  fixarray of size 4 (struct payload)
        assert_eq!(bytes[0], 0x81);
        assert_eq!(bytes[1], 0xAB, "expected fixstr length 11 (PaneCapture)");
        assert_eq!(
            &bytes[2..13],
            b"PaneCapture",
            "variant NAME bytes must stay 'PaneCapture' on the wire"
        );
        assert_eq!(
            bytes[13], 0x94,
            "struct payload must be fixarray of size 4 \
             (session_name, pane_id, text, truncated)"
        );

        let decoded: ServerMessage = rmp_serde::from_slice(&bytes).unwrap();
        assert!(matches!(decoded, ServerMessage::PaneCapture { .. }));
    }

    /// `CapturePaneOpts` is serialised as a positional array. Forward-compat
    /// rule: appending a new field with `#[serde(default)]` must let an older
    /// (shorter) array still deserialise.
    #[test]
    fn capture_pane_opts_empty_array_deserialises_to_default() {
        use crate::message::CapturePaneOpts;
        // msgpack fixarray-0: a struct payload from a hypothetical older
        // sender that had zero fields. Must yield Default::default().
        let empty_bytes = [0x90u8];
        let from_empty: CapturePaneOpts = rmp_serde::from_slice(&empty_bytes).unwrap();
        assert_eq!(from_empty.scrollback_rows, 0);
        assert!(!from_empty.join_wrapped);
        assert!(!from_empty.preserve_trailing_spaces);
    }

    #[test]
    fn capture_pane_opts_full_payload_round_trips() {
        use crate::message::CapturePaneOpts;
        let new_full = CapturePaneOpts {
            scrollback_rows: 7,
            join_wrapped: true,
            preserve_trailing_spaces: true,
        };
        let bytes = rmp_serde::to_vec(&new_full).unwrap();
        let decoded: CapturePaneOpts = rmp_serde::from_slice(&bytes).unwrap();
        assert_eq!(decoded.scrollback_rows, 7);
        assert!(decoded.join_wrapped);
        assert!(decoded.preserve_trailing_spaces);
    }

    /// The realistic forward-compat case: a peer one version behind sends a
    /// 2-field array (the original two fields), and we transparently
    /// default the field that didn't exist yet. Hand-crafted msgpack:
    ///   0x92                  fixarray of size 2
    ///   0x07                  positive fixint 7      (scrollback_rows)
    ///   0xC3                  true                   (join_wrapped)
    #[test]
    fn capture_pane_opts_partial_array_defaults_trailing_field() {
        use crate::message::CapturePaneOpts;
        let bytes = [0x92u8, 0x07, 0xC3];
        let decoded: CapturePaneOpts = rmp_serde::from_slice(&bytes).unwrap();
        assert_eq!(decoded.scrollback_rows, 7);
        assert!(decoded.join_wrapped);
        assert!(
            !decoded.preserve_trailing_spaces,
            "trailing field absent from older array must default to false"
        );
    }

    /// Pin the wire-shape of `CapturePaneOpts::default()` so a field
    /// reorder breaks loudly here instead of corrupting peers.
    #[test]
    fn capture_pane_opts_default_wire_shape_pinned() {
        use crate::message::CapturePaneOpts;
        let bytes = rmp_serde::to_vec(&CapturePaneOpts::default()).unwrap();
        // Expected: fixarray-3 [0=scrollback_rows, false, false]
        assert_eq!(
            bytes.as_slice(),
            &[0x93, 0x00, 0xC2, 0xC2],
            "CapturePaneOpts default wire shape drifted — field order or \
             types changed?"
        );
    }

    /// Pin the FIELD ORDER inside `ClientMessage::CapturePane`. The
    /// fixarray-of-size-3 length check elsewhere catches add/remove,
    /// but a silent swap of two same-shape fields (e.g. session_name
    /// and opts, which is an inner struct also serialised as an array)
    /// wouldn't change the outer length — this test would.
    #[test]
    fn client_message_capture_pane_field_order_pinned() {
        let msg = ClientMessage::CapturePane {
            session_name: "s".into(),
            pane_id: 1,
            opts: Default::default(),
        };
        let bytes = rmp_serde::to_vec(&msg).unwrap();
        // Skip variant envelope (0x81 fixmap-1, 0xAB fixstr-11,
        // "CapturePane", 0x93 fixarray-3 → first 14 bytes).
        let payload = &bytes[14..];
        // Expected positional layout:
        //   [0]   0xA1 fixstr-1
        //   [1]   's'                        (session_name = "s")
        //   [2]   0x01                       (pane_id = 1, positive fixint)
        //   [3]   0x93                       (opts = fixarray-3)
        //   [4]   0x00                       (opts.scrollback_rows = 0)
        //   [5]   0xC2                       (opts.join_wrapped = false)
        //   [6]   0xC2                       (opts.preserve_trailing_spaces = false)
        assert_eq!(
            payload,
            &[0xA1, b's', 0x01, 0x93, 0x00, 0xC2, 0xC2],
            "CapturePane field order changed — reordering fields breaks \
             every old peer's positional decode"
        );
    }

    /// Pin the FIELD ORDER inside `ServerMessage::PaneCapture`. The
    /// fixarray-of-size-4 length check elsewhere catches *adding/removing*
    /// fields, but a silent swap of two same-shape fields (e.g. text and
    /// truncated, or session_name and text) wouldn't change the length —
    /// this test would.
    #[test]
    fn server_message_pane_capture_field_order_pinned() {
        let msg = ServerMessage::PaneCapture {
            session_name: "s".into(),
            pane_id: 1,
            text: "".into(),
            truncated: true,
        };
        let bytes = rmp_serde::to_vec(&msg).unwrap();
        // Skip the variant envelope (0x81 fixmap-1, 0xAB fixstr-11,
        // "PaneCapture", 0x94 fixarray-4 → first 14 bytes).
        let payload = &bytes[14..];
        // Expected positional layout:
        //   [0]   0xA1 fixstr-1
        //   [1]   's'                        (session_name = "s")
        //   [2]   0x01                       (pane_id = 1, positive fixint)
        //   [3]   0xA0 fixstr-0              (text = "")
        //   [4]   0xC3                       (truncated = true)
        assert_eq!(
            payload,
            &[0xA1, b's', 0x01, 0xA0, 0xC3],
            "PaneCapture field order changed — reordering fields breaks \
             every old peer's positional decode"
        );
    }

    /// Symmetric forward-compat for the *response* variant: a hypothetical
    /// older server that pre-dates the `truncated` field would emit
    /// `PaneCapture` as a fixarray-3 (session_name, pane_id, text). The
    /// `#[serde(default)]` annotation on `truncated` must default-fill
    /// to `false` so newer clients keep working. Without this test,
    /// we'd only learn the mistake when an old daemon meets a new CLI.
    #[test]
    fn server_message_pane_capture_omitted_truncated_defaults_to_false() {
        // Hand-crafted wire bytes:
        //   0x81                  fixmap-1
        //   0xAB                  fixstr-11
        //   "PaneCapture"         variant name
        //   0x93                  fixarray-3 (no truncated yet)
        //   0xA1 's'              session_name = "s"
        //   0x01                  pane_id = 1
        //   0xA2 'h' 'i'          text = "hi"
        let mut bytes = vec![0x81, 0xAB];
        bytes.extend_from_slice(b"PaneCapture");
        bytes.extend_from_slice(&[0x93, 0xA1, b's', 0x01, 0xA2, b'h', b'i']);

        match rmp_serde::from_slice::<ServerMessage>(&bytes) {
            Ok(ServerMessage::PaneCapture {
                session_name,
                pane_id,
                text,
                truncated,
            }) => {
                assert_eq!(session_name, "s");
                assert_eq!(pane_id, 1);
                assert_eq!(text, "hi");
                assert!(
                    !truncated,
                    "omitted `truncated` field must default to false; \
                     newer client received {truncated}"
                );
            }
            Ok(other) => panic!("expected PaneCapture, got {other:?}"),
            Err(e) => panic!(
                "old-shape response rejected — `#[serde(default)]` on \
                 `truncated` doesn't actually default-fill in rmp-serde \
                 array decoding. err: {e}"
            ),
        }
    }

    /// Forward-compat at the *variant* level: an older sender that pre-dates
    /// the addition of `opts` would send a `CapturePane` payload with only
    /// `session_name` and `pane_id` (fixarray-2). The `#[serde(default)]`
    /// annotation on the `opts` field is meant to handle this. This test
    /// proves whether the annotation actually works for variant payloads —
    /// if rmp-serde rejects the short array, we learn here, not at runtime
    /// against an old peer in production.
    #[test]
    fn client_message_capture_pane_omitted_opts_defaults() {
        // Hand-crafted wire bytes:
        //   0x81                  fixmap-1
        //   0xAB                  fixstr-11
        //   "CapturePane"         variant name
        //   0x92                  fixarray-2 (payload missing `opts`)
        //   0xA1 's'              session_name = "s"
        //   0x01                  pane_id = 1
        let mut bytes = vec![0x81, 0xAB];
        bytes.extend_from_slice(b"CapturePane");
        bytes.extend_from_slice(&[0x92, 0xA1, b's', 0x01]);

        match rmp_serde::from_slice::<ClientMessage>(&bytes) {
            Ok(ClientMessage::CapturePane {
                session_name,
                pane_id,
                opts,
            }) => {
                assert_eq!(session_name, "s");
                assert_eq!(pane_id, 1);
                assert_eq!(opts.scrollback_rows, 0);
                assert!(!opts.join_wrapped);
                assert!(!opts.preserve_trailing_spaces);
            }
            Ok(other) => panic!("expected CapturePane, got {other:?}"),
            Err(e) => panic!(
                "old-shape variant rejected — `#[serde(default)]` on `opts` \
                 doesn't actually default-fill a missing trailing field in \
                 rmp-serde array decoding. err: {e}"
            ),
        }
    }

    // ──── OSC 133 prompt-marks IPC wire format ────────────────────────

    #[test]
    fn client_message_list_prompts_wire_format_pinned() {
        let msg = ClientMessage::ListPrompts {
            session_name: "s".into(),
            pane_id: 1,
        };
        let bytes = rmp_serde::to_vec(&msg).unwrap();
        // Expected layout:
        //   0x81                  fixmap of size 1
        //   0xAB                  fixstr length 11 ("ListPrompts")
        //   "ListPrompts"         11 bytes of UTF-8
        //   0x92                  fixarray of size 2 (session_name, pane_id)
        assert_eq!(bytes[0], 0x81, "expected fixmap-of-size-1 prefix");
        assert_eq!(bytes[1], 0xAB, "expected fixstr length 11 (ListPrompts)");
        assert_eq!(
            &bytes[2..13],
            b"ListPrompts",
            "variant NAME bytes must stay 'ListPrompts' on the wire"
        );
        assert_eq!(
            bytes[13], 0x92,
            "struct payload must be fixarray of size 2"
        );

        let decoded: ClientMessage = rmp_serde::from_slice(&bytes).unwrap();
        assert!(matches!(decoded, ClientMessage::ListPrompts { .. }));
    }

    #[test]
    fn server_message_prompt_list_reply_wire_format_pinned() {
        let msg = ServerMessage::PromptListReply {
            session_name: "s".into(),
            pane_id: 1,
            marks: Vec::new(),
        };
        let bytes = rmp_serde::to_vec(&msg).unwrap();
        // Expected layout:
        //   0x81                  fixmap of size 1
        //   0xB0                  fixstr length 16 ("PromptListReply")
        //                         actually 15 chars -> 0xAF
        //   "PromptListReply"     15 bytes of UTF-8
        //   0x93                  fixarray of size 3 (session_name, pane_id, marks)
        assert_eq!(bytes[0], 0x81);
        assert_eq!(
            bytes[1], 0xAF,
            "expected fixstr length 15 (PromptListReply)"
        );
        assert_eq!(
            &bytes[2..17],
            b"PromptListReply",
            "variant NAME bytes must stay 'PromptListReply' on the wire"
        );
        assert_eq!(
            bytes[17], 0x93,
            "struct payload must be fixarray of size 3 \
             (session_name, pane_id, marks)"
        );

        let decoded: ServerMessage = rmp_serde::from_slice(&bytes).unwrap();
        assert!(matches!(decoded, ServerMessage::PromptListReply { .. }));
    }

    #[test]
    fn prompt_mark_info_default_wire_shape_pinned() {
        use crate::message::PromptMarkInfo;
        let bytes = rmp_serde::to_vec(&PromptMarkInfo::default()).unwrap();
        // Expected: fixarray-5 [u64=0, nil, nil, nil, nil]
        //   0x95                fixarray of size 5
        //   0x00                positive fixint 0   (prompt_line)
        //   0xC0 0xC0 0xC0 0xC0 nil ×4              (output/done/exit/duration)
        assert_eq!(
            bytes.as_slice(),
            &[0x95, 0x00, 0xC0, 0xC0, 0xC0, 0xC0],
            "PromptMarkInfo default wire shape drifted — field order or \
             types changed?"
        );
    }

    #[test]
    fn prompt_mark_info_full_payload_round_trips() {
        use crate::message::PromptMarkInfo;
        let full = PromptMarkInfo {
            prompt_line: 100,
            output_line: Some(101),
            done_line: Some(105),
            exit_code: Some(0),
            duration_ms: Some(42),
        };
        let bytes = rmp_serde::to_vec(&full).unwrap();
        let decoded: PromptMarkInfo = rmp_serde::from_slice(&bytes).unwrap();
        assert_eq!(decoded, full);
    }

    #[test]
    fn prompt_mark_info_partial_array_defaults_trailing_fields() {
        use crate::message::PromptMarkInfo;
        // An older sender that didn't have `duration_ms` yet emits a 4-array.
        // The trailing field must default to None.
        //   0x94                  fixarray-4
        //   0x64                  positive fixint 100  (prompt_line)
        //   0xC0                  nil                 (output_line=None)
        //   0xC0                  nil                 (done_line=None)
        //   0xD1 0x00 0x2A        int16 42 (exit_code=Some(42))
        let bytes = [0x94u8, 0x64, 0xC0, 0xC0, 0xD1, 0x00, 0x2A];
        let decoded: PromptMarkInfo = rmp_serde::from_slice(&bytes).unwrap();
        assert_eq!(decoded.prompt_line, 100);
        assert_eq!(decoded.output_line, None);
        assert_eq!(decoded.done_line, None);
        assert_eq!(decoded.exit_code, Some(42));
        assert_eq!(decoded.duration_ms, None);
    }
}
