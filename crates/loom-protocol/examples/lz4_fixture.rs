//! Emit hex-encoded LZ4 fixtures so the TS decoder can be tested
//! against bytes produced by `lz4_flex::compress` — the exact encoder
//! the server uses. Avoids depending on a JS LZ4 encoder whose output
//! format may diverge from raw-block expectations.
//!
//! ```sh
//! cargo run -p loom-protocol --example lz4_fixture > \
//!     web/packages/loom-codec/src/__fixtures__/lz4.json
//! ```

fn main() {
    // Mix of cases: short repeating data (compressible), medium random
    // data (less compressible), and an exactly-1024-byte payload.
    let cases = [
        (
            "compressible_text",
            b"the quick brown fox jumps over the lazy dog ".repeat(20),
        ),
        ("ascii_alphabet", b"abcdefghijklmnopqrstuvwxyz".repeat(40).to_vec()),
        ("medium_pattern", {
            let mut v = Vec::with_capacity(1024);
            for i in 0..1024u32 {
                v.push(((i * 37) ^ (i >> 3)) as u8);
            }
            v
        }),
    ];

    let mut json_cases: Vec<String> = Vec::new();
    for (name, original) in &cases {
        let compressed = lz4_flex::compress(original);
        let mut payload = Vec::with_capacity(4 + compressed.len());
        payload.extend_from_slice(&(original.len() as u32).to_le_bytes());
        payload.extend_from_slice(&compressed);
        json_cases.push(format!(
            "  {{\"name\": {n}, \"original_hex\": \"{o}\", \"payload_hex\": \"{p}\"}}",
            n = json_string(name),
            o = bytes_to_hex(original),
            p = bytes_to_hex(&payload),
        ));
    }
    println!("[\n{}\n]", json_cases.join(",\n"));
}

fn bytes_to_hex(b: &[u8]) -> String {
    let mut s = String::with_capacity(b.len() * 2);
    for &byte in b {
        s.push_str(&format!("{byte:02x}"));
    }
    s
}

fn json_string(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}
