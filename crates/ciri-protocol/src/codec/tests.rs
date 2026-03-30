use super::frame::{MAX_FRAME_LEN, TAG_SERVER_MSG, build_frame};
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

// ─── Frame-level roundtrip ──────────────────────────────────────

#[tokio::test]
async fn frame_roundtrip_client_msg() {
    let msg = ClientMessage::Input {
        pane_id: 1,
        data: b"hello".to_vec(),
    };
    let mut buf = Vec::new();
    encode_client_msg(&mut buf, &msg).await.unwrap();
    let frame = read_frame(&mut &buf[..]).await.unwrap();
    match frame {
        Frame::ClientMsg(ClientMessage::Input { pane_id, data }) => {
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
    payload.extend_from_slice(&1u64.to_le_bytes());
    payload.extend_from_slice(&2u64.to_le_bytes());
    payload.extend_from_slice(&0i16.to_le_bytes());
    payload.extend_from_slice(&0u16.to_le_bytes());
    payload.push(0);
    payload.push(0);
    payload.extend_from_slice(&80u16.to_le_bytes());
    payload.extend_from_slice(&1u16.to_le_bytes());
    payload.extend_from_slice(&3u16.to_le_bytes());
    payload.extend_from_slice(&5u16.to_le_bytes());
    payload.extend_from_slice(&4u16.to_le_bytes());
    payload.extend_from_slice(&0u32.to_le_bytes());

    let err = decode_cell_delta_borrowed(payload).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    assert!(err.to_string().contains("left > right"));
}

#[test]
fn decode_cell_delta_rejects_truncated_region_data() {
    let mut payload = Vec::new();
    payload.extend_from_slice(&1u64.to_le_bytes());
    payload.extend_from_slice(&2u64.to_le_bytes());
    payload.extend_from_slice(&0i16.to_le_bytes());
    payload.extend_from_slice(&0u16.to_le_bytes());
    payload.push(0);
    payload.push(0);
    payload.extend_from_slice(&80u16.to_le_bytes());
    payload.extend_from_slice(&1u16.to_le_bytes());
    payload.extend_from_slice(&3u16.to_le_bytes());
    payload.extend_from_slice(&4u16.to_le_bytes());
    payload.extend_from_slice(&6u16.to_le_bytes());
    payload.extend_from_slice(&4u32.to_le_bytes());
    payload.extend_from_slice(&[OP_END]);

    let err = decode_cell_delta_borrowed(payload).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    assert!(err.to_string().contains("truncated SM data"));
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
    frame.extend_from_slice(&(MAX_FRAME_LEN + 1).to_le_bytes());
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
            format: "png".to_string(),
            data: vec![1, 2, 3, 4],
        },
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
