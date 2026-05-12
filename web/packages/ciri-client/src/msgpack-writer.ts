// Minimal msgpack writer tuned for ciritty's wire format.
//
// `@msgpack/msgpack` encodes JS values without type hints: a `Number`
// is emitted as msgpack int when `Number.isInteger(n)` and as float
// otherwise, so a struct field typed `f64` carrying the value `320.0`
// goes onto the wire as a uint16 (3 bytes) instead of an f64 (9 bytes).
// rmp_serde's deserializer accepts that and still produces a Rust
// `f64`, so it's server-safe — but it breaks byte-identical round
// trips against Rust-generated fixtures, which masks any future
// encoder regression behind "well, msgpack canonicalizes that way".
//
// This writer takes explicit type hints from the schema and emits the
// matching wire shape every time. It also picks the smallest-int form
// for u64/i64 values (Rust does the same) so wire bytes stay
// byte-identical to `rmp_serde::to_vec`.

const TEXT_ENCODER = new TextEncoder();

export class MsgpackWriter {
  private buf: Uint8Array;
  private view: DataView;
  private pos = 0;

  constructor(initialCapacity = 256) {
    this.buf = new Uint8Array(initialCapacity);
    this.view = new DataView(this.buf.buffer);
  }

  /** Take ownership of the written bytes. Resets internal state — do
   *  not reuse the writer after calling this. */
  finish(): Uint8Array {
    return this.buf.slice(0, this.pos);
  }

  // ─── Primitive writers ──────────────────────────────────────────

  writeNil(): void {
    this.ensure(1);
    this.buf[this.pos++] = 0xc0;
  }

  writeBool(b: boolean): void {
    this.ensure(1);
    this.buf[this.pos++] = b ? 0xc3 : 0xc2;
  }

  /** Write a signed integer with rmp_serde's "smallest form" choice:
   *  positive-fixint / negative-fixint when possible, else the smallest
   *  msgpack int marker that fits. Accepts `number` or `bigint`. */
  writeSigned(value: number | bigint): void {
    const n = typeof value === "bigint" ? value : BigInt(Math.trunc(value));
    if (n >= 0n) {
      this.writeUnsigned(n);
      return;
    }
    if (n >= -32n) {
      // negative fixint: 0xe0..0xff (5-bit negative range)
      this.ensure(1);
      this.buf[this.pos++] = 0xe0 | (Number(n + 32n) & 0x1f);
      return;
    }
    if (n >= -0x80n) {
      this.ensure(2);
      this.buf[this.pos++] = 0xd0;
      this.view.setInt8(this.pos, Number(n));
      this.pos += 1;
      return;
    }
    if (n >= -0x8000n) {
      this.ensure(3);
      this.buf[this.pos++] = 0xd1;
      this.view.setInt16(this.pos, Number(n), false);
      this.pos += 2;
      return;
    }
    if (n >= -0x80000000n) {
      this.ensure(5);
      this.buf[this.pos++] = 0xd2;
      this.view.setInt32(this.pos, Number(n), false);
      this.pos += 4;
      return;
    }
    this.ensure(9);
    this.buf[this.pos++] = 0xd3;
    this.view.setBigInt64(this.pos, n, false);
    this.pos += 8;
  }

  /** Write an unsigned integer in the smallest msgpack form that fits. */
  writeUnsigned(value: number | bigint): void {
    const n = typeof value === "bigint" ? value : BigInt(Math.trunc(value));
    if (n < 0n) {
      throw new RangeError(`writeUnsigned given negative value: ${n}`);
    }
    if (n <= 0x7fn) {
      // positive fixint: 0x00..0x7f
      this.ensure(1);
      this.buf[this.pos++] = Number(n);
      return;
    }
    if (n <= 0xffn) {
      this.ensure(2);
      this.buf[this.pos++] = 0xcc;
      this.buf[this.pos++] = Number(n);
      return;
    }
    if (n <= 0xffffn) {
      this.ensure(3);
      this.buf[this.pos++] = 0xcd;
      this.view.setUint16(this.pos, Number(n), false);
      this.pos += 2;
      return;
    }
    if (n <= 0xffffffffn) {
      this.ensure(5);
      this.buf[this.pos++] = 0xce;
      this.view.setUint32(this.pos, Number(n), false);
      this.pos += 4;
      return;
    }
    this.ensure(9);
    this.buf[this.pos++] = 0xcf;
    this.view.setBigUint64(this.pos, n, false);
    this.pos += 8;
  }

  /** Always 5 bytes (`0xca` + 4 BE) — never auto-narrows to int. */
  writeFloat32(value: number): void {
    this.ensure(5);
    this.buf[this.pos++] = 0xca;
    this.view.setFloat32(this.pos, value, false);
    this.pos += 4;
  }

  /** Always 9 bytes (`0xcb` + 8 BE) — never auto-narrows to int. */
  writeFloat64(value: number): void {
    this.ensure(9);
    this.buf[this.pos++] = 0xcb;
    this.view.setFloat64(this.pos, value, false);
    this.pos += 8;
  }

  writeString(s: string): void {
    const bytes = TEXT_ENCODER.encode(s);
    this.writeStringHeader(bytes.length);
    this.writeRawBytes(bytes);
  }

  /** Emit msgpack `bin` (typed-array path) — currently unused, but
   *  kept here so we don't have to retro-fit if a future field gains
   *  `#[serde(with = "serde_bytes")]`. */
  writeBin(bytes: Uint8Array): void {
    const n = bytes.length;
    if (n <= 0xff) {
      this.ensure(2);
      this.buf[this.pos++] = 0xc4;
      this.buf[this.pos++] = n;
    } else if (n <= 0xffff) {
      this.ensure(3);
      this.buf[this.pos++] = 0xc5;
      this.view.setUint16(this.pos, n, false);
      this.pos += 2;
    } else {
      this.ensure(5);
      this.buf[this.pos++] = 0xc6;
      this.view.setUint32(this.pos, n, false);
      this.pos += 4;
    }
    this.writeRawBytes(bytes);
  }

  writeArrayHeader(len: number): void {
    if (len <= 15) {
      this.ensure(1);
      this.buf[this.pos++] = 0x90 | len;
    } else if (len <= 0xffff) {
      this.ensure(3);
      this.buf[this.pos++] = 0xdc;
      this.view.setUint16(this.pos, len, false);
      this.pos += 2;
    } else {
      this.ensure(5);
      this.buf[this.pos++] = 0xdd;
      this.view.setUint32(this.pos, len, false);
      this.pos += 4;
    }
  }

  writeMapHeader(len: number): void {
    if (len <= 15) {
      this.ensure(1);
      this.buf[this.pos++] = 0x80 | len;
    } else if (len <= 0xffff) {
      this.ensure(3);
      this.buf[this.pos++] = 0xde;
      this.view.setUint16(this.pos, len, false);
      this.pos += 2;
    } else {
      this.ensure(5);
      this.buf[this.pos++] = 0xdf;
      this.view.setUint32(this.pos, len, false);
      this.pos += 4;
    }
  }

  // ─── Internal ────────────────────────────────────────────────────

  private writeStringHeader(len: number): void {
    if (len <= 31) {
      this.ensure(1);
      this.buf[this.pos++] = 0xa0 | len;
    } else if (len <= 0xff) {
      this.ensure(2);
      this.buf[this.pos++] = 0xd9;
      this.buf[this.pos++] = len;
    } else if (len <= 0xffff) {
      this.ensure(3);
      this.buf[this.pos++] = 0xda;
      this.view.setUint16(this.pos, len, false);
      this.pos += 2;
    } else {
      this.ensure(5);
      this.buf[this.pos++] = 0xdb;
      this.view.setUint32(this.pos, len, false);
      this.pos += 4;
    }
  }

  private writeRawBytes(bytes: Uint8Array): void {
    this.ensure(bytes.length);
    this.buf.set(bytes, this.pos);
    this.pos += bytes.length;
  }

  private ensure(extra: number): void {
    const needed = this.pos + extra;
    if (needed <= this.buf.length) return;
    // Geometric growth: doubling keeps the amortized cost low while
    // never wasting more than a factor of 2.
    let cap = this.buf.length;
    while (cap < needed) cap *= 2;
    const next = new Uint8Array(cap);
    next.set(this.buf.subarray(0, this.pos), 0);
    this.buf = next;
    this.view = new DataView(this.buf.buffer);
  }
}
