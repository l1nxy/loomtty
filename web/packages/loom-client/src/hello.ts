// ClientHello / ServerHello bytes — the pre-frame handshake the loom
// server reads before either side speaks the framed protocol.
//
// Wire format (mirrors `crates/loom-protocol/src/codec/handshake.rs`):
//
//   ClientHello:
//     [u8;  4] magic = "LOOM"
//     [u32 LE] pkg_version (major<<24 | minor<<16 | patch)
//     [u8]     wire_version
//     [u16 LE] session_name_len (bytes, not chars)
//     [u8;  N] session_name (UTF-8, 0 ≤ N ≤ 255)
//     [u32 LE] width  (px, 1..=16384)
//     [u32 LE] height (px, 1..=16384)
//     [f32 LE] cell_width  (0 < x ≤ 200, finite)
//     [f32 LE] cell_height (0 < x ≤ 200, finite)
//
//   ServerHello (8 bytes):
//     [u8;  4] magic = "LOOM"
//     [u32 LE] pkg_version
//
// `WIRE_PROTOCOL_VERSION` and `LOOM_PKG_VERSION` come from the
// auto-generated `__generated__/fixtures.ts` so a workspace version
// bump (or a wire format break) shows up here without manual edits.

import {
  LOOM_PKG_VERSION,
  SERVER_HELLO_LEN,
  WIRE_PROTOCOL_VERSION,
} from "./__generated__/fixtures.js";

// Typed arrays cannot be frozen by `Object.freeze` (the engine rejects
// freezing a backing store with elements), so this is left as a `const`
// reference instead. Treat it as read-only — never mutate.
const HANDSHAKE_MAGIC = new Uint8Array([0x43, 0x49, 0x52, 0x49]); // "LOOM"
const CLIENT_HELLO_HEADER_LEN = 11;
const CLIENT_HELLO_FIXED_FIELDS_LEN = 16;
const MAX_SESSION_NAME_LEN = 255;

const MAX_VIEWPORT_DIM = 16384;
const MAX_CELL_DIM = 200;

export interface ClientHello {
  sessionName: string;
  width: number;
  height: number;
  cellWidth: number;
  cellHeight: number;
}

export interface ServerHelloInfo {
  /** Packed semver `(major<<24 | minor<<16 | patch)` reported by the peer. */
  peerVersion: number;
  /** Comparison result against the locally-bundled `LOOM_PKG_VERSION`. */
  compat: VersionCompat;
}

export type VersionCompat =
  | { kind: "exact"; version: string }
  | { kind: "patch-mismatch"; peer: string; local: string }
  | { kind: "minor-mismatch"; peer: string; local: string };

export class HandshakeError extends Error {
  // `cause` is forwarded to `Error`'s ES2022 second-argument options
  // so callers can inspect it via `err.cause` per the standard. The
  // field itself is declared on `Error` already, so we don't redeclare.
  constructor(message: string, cause?: unknown) {
    super(message, cause === undefined ? undefined : { cause });
    this.name = "HandshakeError";
  }
}

/** Build the ClientHello byte sequence. Validates the same invariants
 *  the server's `validate_viewport_dims` enforces; rejects oversize
 *  session names. Throws `HandshakeError` on any violation. */
export function encodeClientHello(hello: ClientHello): Uint8Array {
  validateClientHello(hello);
  const nameBytes = new TextEncoder().encode(hello.sessionName);
  if (nameBytes.length > MAX_SESSION_NAME_LEN) {
    throw new HandshakeError(
      `session name is ${nameBytes.length} bytes; max ${MAX_SESSION_NAME_LEN}`,
    );
  }

  const total =
    CLIENT_HELLO_HEADER_LEN + nameBytes.length + CLIENT_HELLO_FIXED_FIELDS_LEN;
  const buf = new Uint8Array(total);
  const view = new DataView(buf.buffer);
  let p = 0;

  buf.set(HANDSHAKE_MAGIC, p);
  p += 4;
  view.setUint32(p, LOOM_PKG_VERSION, /* littleEndian */ true);
  p += 4;
  buf[p] = WIRE_PROTOCOL_VERSION;
  p += 1;
  view.setUint16(p, nameBytes.length, true);
  p += 2;
  buf.set(nameBytes, p);
  p += nameBytes.length;
  view.setUint32(p, hello.width, true);
  p += 4;
  view.setUint32(p, hello.height, true);
  p += 4;
  view.setFloat32(p, hello.cellWidth, true);
  p += 4;
  view.setFloat32(p, hello.cellHeight, true);
  p += 4;
  if (p !== total) {
    throw new HandshakeError(
      `internal: ClientHello wrote ${p} bytes, expected ${total}`,
    );
  }
  return buf;
}

/** Decode a ClientHello byte sequence — the inverse of
 *  [`encodeClientHello`]. Mirrors `parse_client_hello_header` +
 *  `decode_client_hello_parts` on the Rust side
 *  (`crates/loom-protocol/src/codec/handshake.rs`).
 *
 *  Useful for dev tooling (the bridge's diagnostic log, replay
 *  fixtures, round-trip tests) — production clients don't decode
 *  their own ClientHello.
 *
 *  Throws `HandshakeError` on bad magic, wrong wire version,
 *  truncated payload, oversize session name, non-UTF-8 session name,
 *  or any viewport / cell dim that fails the same validation
 *  `encodeClientHello` applies on the way out. */
export function decodeClientHello(bytes: Uint8Array): {
  hello: ClientHello;
  peerVersion: number;
  wireVersion: number;
} {
  if (bytes.length < CLIENT_HELLO_HEADER_LEN) {
    throw new HandshakeError(
      `ClientHello truncated: need at least ${CLIENT_HELLO_HEADER_LEN} header bytes, got ${bytes.length}`,
    );
  }
  for (let i = 0; i < 4; i += 1) {
    if (bytes[i] !== HANDSHAKE_MAGIC[i]) {
      throw new HandshakeError(
        `ClientHello: bad magic (got 0x${(bytes[i] ?? 0).toString(16)} at offset ${i})`,
      );
    }
  }
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const peerVersion = view.getUint32(4, true);
  const wireVersion = bytes[8]!;
  if (wireVersion !== WIRE_PROTOCOL_VERSION) {
    throw new HandshakeError(
      `incompatible wire protocol: peer=${wireVersion}, local=${WIRE_PROTOCOL_VERSION}`,
    );
  }
  const nameLen = view.getUint16(9, true);
  if (nameLen > MAX_SESSION_NAME_LEN) {
    throw new HandshakeError(
      `ClientHello: session name length ${nameLen} exceeds max ${MAX_SESSION_NAME_LEN}`,
    );
  }
  const total = CLIENT_HELLO_HEADER_LEN + nameLen + CLIENT_HELLO_FIXED_FIELDS_LEN;
  if (bytes.length < total) {
    throw new HandshakeError(
      `ClientHello truncated: declared length ${total}, got ${bytes.length}`,
    );
  }
  let sessionName: string;
  try {
    sessionName = new TextDecoder("utf-8", { fatal: true }).decode(
      bytes.subarray(CLIENT_HELLO_HEADER_LEN, CLIENT_HELLO_HEADER_LEN + nameLen),
    );
  } catch (e) {
    throw new HandshakeError("ClientHello: invalid session name utf8", e);
  }
  let p = CLIENT_HELLO_HEADER_LEN + nameLen;
  const width = view.getUint32(p, true); p += 4;
  const height = view.getUint32(p, true); p += 4;
  const cellWidth = view.getFloat32(p, true); p += 4;
  const cellHeight = view.getFloat32(p, true); p += 4;
  const hello: ClientHello = { sessionName, width, height, cellWidth, cellHeight };
  // Validate viewport / cell dims only — mirroring Rust's
  // `decode_client_hello_parts`, which leaves session-name char-set
  // validation to the caller (`validate_name` runs separately in
  // `connection.rs`). Reusing `validateClientHello` would reject the
  // legitimate `__control__` IPC session, plus any future names the
  // server admits, on a path that's supposed to be lenient enough
  // for diagnostics and replay fixtures.
  validateViewportDims(hello);
  return { hello, peerVersion, wireVersion };
}

/** Decode the 8-byte ServerHello frame. Verifies magic and returns the
 *  peer's pkg_version along with a `VersionCompat` comparison against
 *  the locally-bundled `LOOM_PKG_VERSION`. Same compatibility rules as
 *  the server: a major mismatch throws; a minor mismatch is reported
 *  but not fatal (caller decides). */
export function decodeServerHello(bytes: Uint8Array): ServerHelloInfo {
  if (bytes.length !== SERVER_HELLO_LEN) {
    throw new HandshakeError(
      `ServerHello must be ${SERVER_HELLO_LEN} bytes; got ${bytes.length}`,
    );
  }
  for (let i = 0; i < 4; i += 1) {
    if (bytes[i] !== HANDSHAKE_MAGIC[i]) {
      throw new HandshakeError(
        `ServerHello: bad magic (got 0x${(bytes[i] ?? 0).toString(16)} at offset ${i})`,
      );
    }
  }
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const peerVersion = view.getUint32(4, true);
  return { peerVersion, compat: compareVersions(peerVersion) };
}

function compareVersions(peer: number): VersionCompat {
  const peerMajor = (peer >>> 24) & 0xff;
  const peerMinor = (peer >>> 16) & 0xff;
  const localMajor = (LOOM_PKG_VERSION >>> 24) & 0xff;
  const localMinor = (LOOM_PKG_VERSION >>> 16) & 0xff;
  if (peerMajor !== localMajor) {
    throw new HandshakeError(
      `incompatible major version: peer=${formatVersion(peer)}, local=${formatVersion(LOOM_PKG_VERSION)}`,
    );
  }
  if (peer === LOOM_PKG_VERSION) {
    return { kind: "exact", version: formatVersion(peer) };
  }
  if (peerMinor !== localMinor) {
    return {
      kind: "minor-mismatch",
      peer: formatVersion(peer),
      local: formatVersion(LOOM_PKG_VERSION),
    };
  }
  return {
    kind: "patch-mismatch",
    peer: formatVersion(peer),
    local: formatVersion(LOOM_PKG_VERSION),
  };
}

function formatVersion(v: number): string {
  const major = (v >>> 24) & 0xff;
  const minor = (v >>> 16) & 0xff;
  const patch = v & 0xffff;
  return `${major}.${minor}.${patch}`;
}

function validateClientHello(h: ClientHello): void {
  validateSessionName(h.sessionName);
  validateViewportDims(h);
}

function validateViewportDims(h: ClientHello): void {
  if (!Number.isFinite(h.cellWidth) || h.cellWidth <= 0 || h.cellWidth > MAX_CELL_DIM) {
    throw new HandshakeError(`invalid cellWidth: ${h.cellWidth}`);
  }
  if (!Number.isFinite(h.cellHeight) || h.cellHeight <= 0 || h.cellHeight > MAX_CELL_DIM) {
    throw new HandshakeError(`invalid cellHeight: ${h.cellHeight}`);
  }
  if (
    !Number.isInteger(h.width) ||
    h.width <= 0 ||
    h.width > MAX_VIEWPORT_DIM
  ) {
    throw new HandshakeError(`invalid width: ${h.width}`);
  }
  if (
    !Number.isInteger(h.height) ||
    h.height <= 0 ||
    h.height > MAX_VIEWPORT_DIM
  ) {
    throw new HandshakeError(`invalid height: ${h.height}`);
  }
}

/** The server's IPC / CLI control path. Browser callers driving
 *  `ListSessions`, `RunCommand`, etc. without attaching to a pane
 *  go through this name, which the Rust server (`connection.rs`)
 *  explicitly exempts from the lowercase-ASCII rule. */
const CONTROL_SESSION = "__control__";

/** Handshake sentinel asking the server to auto-attach to the
 *  most-recently-used session (or create a fresh one). Like
 *  `__control__`, it's `__`-prefixed and so exempt from the
 *  lowercase-ASCII rule below. Mirrors `AUTO_SESSION` in
 *  `crates/loom-server/src/daemon/connection.rs`. */
const AUTO_SESSION = "__auto__";

/** Mirror of `loom_session::names::validate_name` (the Rust server
 *  runs this check before sending ServerHello). Without it, a client
 *  with a bad name just sees the connection close cleanly with no
 *  diagnostic — we want a local error so the caller's UI can show
 *  the offending characters. The control/auto-session escapes mirror
 *  `crates/loom-server/src/daemon/connection.rs`. */
function validateSessionName(name: string): void {
  if (name === CONTROL_SESSION || name === AUTO_SESSION) return;
  if (name.length === 0) {
    throw new HandshakeError("session name cannot be empty");
  }
  // The server's validator counts bytes (`name.len()`); UTF-8 strings
  // here may have multi-byte characters, but the lowercase-ASCII rule
  // below rejects those anyway. Use `.length` (UTF-16 units) as a
  // closer-to-the-server proxy for short ASCII names; for longer
  // multi-byte names the character rule trips first.
  if (name.length > 64) {
    throw new HandshakeError(`session name too long (max 64 chars), got ${name.length}`);
  }
  if (name.startsWith("-")) {
    throw new HandshakeError("session name cannot start with '-'");
  }
  for (let i = 0; i < name.length; i += 1) {
    const ch = name.charCodeAt(i);
    const isLower = ch >= 0x61 && ch <= 0x7a; // a-z
    const isDigit = ch >= 0x30 && ch <= 0x39; // 0-9
    const isHyphen = ch === 0x2d;
    if (!isLower && !isDigit && !isHyphen) {
      throw new HandshakeError(
        `session name can only contain lowercase letters, digits, and hyphens (got ${JSON.stringify(name[i])} at offset ${i})`,
      );
    }
  }
}
