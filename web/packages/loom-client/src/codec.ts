// Schema-driven msgpack codec for loomtty control messages.
//
// rmp_serde's compact externally-tagged enum layout (see the wire-format
// probe in `crates/loom-protocol/examples/dump_messages.rs`):
//
//   - Unit variants: a bare msgpack string (`"Attach"`).
//   - Payload variants: a one-entry msgpack map `{"Variant": <body>}`.
//     The body is a msgpack array whose elements are the struct fields
//     in declaration order — there are NO field names on the wire.
//   - Nested struct types: same positional array layout.
//   - `Vec<u8>` (default serde) is a msgpack ARRAY of fixints, not
//     msgpack `bin`. The TS side surfaces these as `Uint8Array` for
//     ergonomics and the codec converts to/from `number[]` at the wire
//     boundary.
//   - `usize` and `u64` collapse to the same wire shape; both decode as
//     `bigint` on the JS side so values above `Number.MAX_SAFE_INTEGER`
//     never silently lose precision.
//
// The schema in `__generated__/schema.ts` is itself derived from
// `serde-reflection`'s introspection of the actual `Serialize` impls,
// so the field order this codec walks is by construction the same one
// rmp_serde reads.

import { decode } from "@msgpack/msgpack";
import {
  REGISTRY,
  type ContainerSpec,
  type FieldSpec,
  type SchemaType,
  type VariantSpec,
} from "./__generated__/schema.js";
import type { ClientMessage, ServerMessage } from "./__generated__/types.js";
import { MsgpackWriter } from "./msgpack-writer.js";

export class CodecError extends Error {
  constructor(message: string, cause?: unknown) {
    super(message, cause === undefined ? undefined : { cause });
    this.name = "CodecError";
  }
}

// ─── Public entry points ────────────────────────────────────────────

/** Encode a `ClientMessage` discriminated-union value into msgpack
 *  bytes matching the rmp_serde wire layout the server reads. */
export function encodeClientMessage(msg: ClientMessage): Uint8Array {
  const w = new MsgpackWriter();
  encodeContainerInto(w, msg, requireContainer("ClientMessage"));
  return w.finish();
}

/** Decode msgpack bytes (typically a `TAG_SERVER_MSG` frame payload)
 *  into a typed `ServerMessage`. Throws `CodecError` on any shape
 *  mismatch or unknown variant. */
export function decodeServerMessage(bytes: Uint8Array): ServerMessage {
  let raw: unknown;
  try {
    raw = decode(bytes, { useBigInt64: true });
  } catch (e) {
    throw new CodecError("malformed msgpack payload", e);
  }
  return decodeContainer(raw, requireContainer("ServerMessage")) as ServerMessage;
}

// ─── Encode walkers (write into MsgpackWriter) ──────────────────────

function encodeContainerInto(
  w: MsgpackWriter,
  value: unknown,
  spec: ContainerSpec,
): void {
  switch (spec.kind) {
    case "unit-struct":
      w.writeNil();
      return;
    case "newtype-struct":
      encodeValueInto(w, value, spec.of);
      return;
    case "tuple-struct": {
      const arr = requireArray(value, "tuple-struct");
      if (arr.length !== spec.of.length) {
        throw new CodecError(
          `tuple-struct arity mismatch: schema=${spec.of.length}, got=${arr.length}`,
        );
      }
      w.writeArrayHeader(arr.length);
      for (let i = 0; i < arr.length; i += 1) encodeValueInto(w, arr[i], spec.of[i]!);
      return;
    }
    case "struct":
      encodeStructFieldsInto(w, value, spec.fields);
      return;
    case "enum":
      encodeEnumVariantInto(w, value, spec.variants);
      return;
    default:
      throw new CodecError(`unhandled container kind: ${(spec as { kind: string }).kind}`);
  }
}

function encodeStructFieldsInto(
  w: MsgpackWriter,
  value: unknown,
  fields: FieldSpec[],
): void {
  const obj = requireObject(value, "struct body");
  w.writeArrayHeader(fields.length);
  for (const f of fields) {
    const v = obj[f.jsName];
    if (v === undefined && f.type.kind !== "option") {
      throw new CodecError(
        `missing required field ${JSON.stringify(f.jsName)} for ${describeSchemaType(f.type)}`,
      );
    }
    encodeValueInto(w, v ?? null, f.type);
  }
}

function encodeEnumVariantInto(
  w: MsgpackWriter,
  value: unknown,
  variants: VariantSpec[],
): void {
  const tagged = requireObject(value, "enum");
  const tag = tagged["tag"];
  if (typeof tag !== "string") {
    throw new CodecError(`enum value missing string \`tag\`; got ${typeofTag(tag)}`);
  }
  const variant = variants.find((v) => v.name === tag);
  if (!variant) {
    throw new CodecError(`unknown variant: ${JSON.stringify(tag)}`);
  }
  if (variant.shape.kind === "unit-struct") {
    // Externally-tagged: a unit variant rides on the wire as a bare
    // msgpack string — no enclosing map.
    w.writeString(variant.name);
    return;
  }
  // Externally-tagged with payload: 1-entry msgpack map keyed by the
  // variant name, body in the schema's container shape. Strip the
  // discriminant from the payload before recursing so a future
  // struct field literally named `tag` (camelCase) can't be
  // shadowed by our discriminant string at encode time.
  const { tag: _discriminant, ...payload } = tagged;
  void _discriminant;
  w.writeMapHeader(1);
  w.writeString(variant.name);
  // For newtype/tuple-struct variant shapes, the TS surface puts the
  // payload under a `value` key (`{ tag: "Foo", value: T }`), but the
  // inner encoder expects T itself, not the wrapper object. Unwrap
  // before recursing. Struct variants keep the named-field shape on
  // both sides.
  if (
    variant.shape.kind === "newtype-struct" ||
    variant.shape.kind === "tuple-struct"
  ) {
    encodeContainerInto(w, payload["value"], variant.shape);
  } else {
    encodeContainerInto(w, payload, variant.shape);
  }
}

function encodeValueInto(w: MsgpackWriter, value: unknown, ty: SchemaType): void {
  switch (ty.kind) {
    case "unit":
      w.writeNil();
      return;
    case "bool":
      if (typeof value !== "boolean") {
        throw new CodecError(`bool expected, got ${typeofTag(value)}`);
      }
      w.writeBool(value);
      return;
    case "i8":
    case "i16":
    case "i32":
    case "i64":
    case "i128":
      w.writeSigned(requireIntLike(value, ty.kind, /* signed */ true));
      return;
    case "u8":
    case "u16":
    case "u32":
    case "u64":
    case "u128":
      w.writeUnsigned(requireIntLike(value, ty.kind, /* signed */ false));
      return;
    case "f32":
      if (typeof value !== "number") {
        throw new CodecError(`${ty.kind} expected number, got ${typeofTag(value)}`);
      }
      if (!Number.isFinite(value)) {
        // `NaN` / `Infinity` are valid msgpack floats but corrupt
        // every Rust layout-math consumer (column proportions,
        // weights, cell metrics) the moment they land. The integer
        // path already rejects out-of-range / non-integer values;
        // mirror that here so a bad caller can't poison the server
        // state via a layout message.
        throw new CodecError(`${ty.kind} expected finite number, got ${value}`);
      }
      w.writeFloat32(value);
      return;
    case "f64":
      if (typeof value !== "number") {
        throw new CodecError(`${ty.kind} expected number, got ${typeofTag(value)}`);
      }
      if (!Number.isFinite(value)) {
        throw new CodecError(`${ty.kind} expected finite number, got ${value}`);
      }
      w.writeFloat64(value);
      return;
    case "char":
    case "str":
      if (typeof value !== "string") {
        throw new CodecError(`str expected, got ${typeofTag(value)}`);
      }
      w.writeString(value);
      return;
    case "bytes":
      // `serde_bytes`-tagged Rust types — we don't currently emit any
      // but keep the path for forward compatibility.
      if (!(value instanceof Uint8Array)) {
        throw new CodecError(`bytes expected Uint8Array, got ${typeofTag(value)}`);
      }
      w.writeBin(value);
      return;
    case "ref":
      encodeContainerInto(w, value, requireContainer(ty.name));
      return;
    case "option":
      if (value === null || value === undefined) {
        w.writeNil();
      } else {
        encodeValueInto(w, value, ty.of);
      }
      return;
    case "seq": {
      if (ty.of.kind === "u8" && value instanceof Uint8Array) {
        // Wire shape for `Vec<u8>`: msgpack array of fixints (each
        // element is in [0, 255], so fixint always fits in 1 byte).
        w.writeArrayHeader(value.length);
        for (const b of value) w.writeUnsigned(b);
        return;
      }
      const arr = requireArray(value, "seq");
      w.writeArrayHeader(arr.length);
      for (const v of arr) encodeValueInto(w, v, ty.of);
      return;
    }
    case "tuple": {
      const arr = requireArray(value, "tuple");
      if (arr.length !== ty.of.length) {
        throw new CodecError(
          `tuple arity mismatch: schema=${ty.of.length}, got=${arr.length}`,
        );
      }
      w.writeArrayHeader(arr.length);
      for (let i = 0; i < arr.length; i += 1) encodeValueInto(w, arr[i], ty.of[i]!);
      return;
    }
    case "tuple-array": {
      const arr = requireArray(value, "tuple-array");
      if (arr.length !== ty.size) {
        throw new CodecError(
          `tuple-array size mismatch: schema=${ty.size}, got=${arr.length}`,
        );
      }
      w.writeArrayHeader(arr.length);
      for (const v of arr) encodeValueInto(w, v, ty.of);
      return;
    }
    case "map": {
      if (!(value instanceof Map)) {
        throw new CodecError(`map expected Map, got ${typeofTag(value)}`);
      }
      w.writeMapHeader(value.size);
      for (const [k, v] of value) {
        encodeValueInto(w, k, ty.key);
        encodeValueInto(w, v, ty.value);
      }
      return;
    }
    default:
      throw new CodecError(`unhandled schema kind: ${(ty as { kind: string }).kind}`);
  }
}

// ─── Decode walkers ─────────────────────────────────────────────────

function decodeContainer(raw: unknown, spec: ContainerSpec): unknown {
  switch (spec.kind) {
    case "unit-struct":
      return null;
    case "newtype-struct":
      return decodeValue(raw, spec.of);
    case "tuple-struct": {
      const arr = requireArray(raw, "tuple-struct");
      if (arr.length !== spec.of.length) {
        throw new CodecError(
          `tuple-struct arity mismatch on wire: schema=${spec.of.length}, got=${arr.length}`,
        );
      }
      return spec.of.map((ty, i) => decodeValue(arr[i], ty));
    }
    case "struct":
      return decodeStructFields(raw, spec.fields);
    case "enum":
      return decodeEnumVariant(raw, spec.variants);
    default:
      throw new CodecError(`unhandled container kind: ${(spec as { kind: string }).kind}`);
  }
}

function decodeStructFields(raw: unknown, fields: FieldSpec[]): Record<string, unknown> {
  const arr = requireArray(raw, "struct body");
  if (arr.length !== fields.length) {
    throw new CodecError(
      `struct arity mismatch on wire: schema=${fields.length}, got=${arr.length}`,
    );
  }
  const out: Record<string, unknown> = {};
  for (let i = 0; i < fields.length; i += 1) {
    const f = fields[i]!;
    out[f.jsName] = decodeValue(arr[i], f.type);
  }
  return out;
}

function decodeEnumVariant(raw: unknown, variants: VariantSpec[]): Record<string, unknown> {
  if (typeof raw === "string") {
    // Externally-tagged unit variant.
    const variant = variants.find((v) => v.name === raw);
    if (!variant) {
      throw new CodecError(`unknown unit variant on wire: ${JSON.stringify(raw)}`);
    }
    if (variant.shape.kind !== "unit-struct") {
      throw new CodecError(
        `wire said unit variant ${JSON.stringify(raw)} but schema expects ${variant.shape.kind}`,
      );
    }
    return { tag: variant.name };
  }
  // Payload variant: 1-entry map `{ Variant: body }`. @msgpack/msgpack
  // decodes msgpack maps with string keys to plain objects by default.
  const obj = requireObject(raw, "enum payload");
  const keys = Object.keys(obj);
  if (keys.length !== 1) {
    throw new CodecError(
      `enum payload must be a 1-entry map, got ${keys.length} entries`,
    );
  }
  const tag = keys[0]!;
  const variant = variants.find((v) => v.name === tag);
  if (!variant) {
    throw new CodecError(`unknown variant on wire: ${JSON.stringify(tag)}`);
  }
  if (variant.shape.kind === "unit-struct") {
    throw new CodecError(
      `wire said payload variant ${JSON.stringify(tag)} but schema expects unit`,
    );
  }
  const decoded = decodeContainer(obj[tag], variant.shape);
  // Promote the discriminant onto the body. Struct bodies decode to a
  // plain object, but tuple-struct / newtype-struct decode to an array
  // or scalar — pack them under `value` so the caller's typed
  // discriminated union still works (`{ tag, value }`).
  if (
    variant.shape.kind === "struct" &&
    decoded !== null &&
    typeof decoded === "object" &&
    !Array.isArray(decoded)
  ) {
    return { tag, ...(decoded as Record<string, unknown>) };
  }
  return { tag, value: decoded } as Record<string, unknown>;
}

function decodeValue(raw: unknown, ty: SchemaType): unknown {
  switch (ty.kind) {
    case "unit":
      // rmp_serde encodes `()` as `nil`; we surface as `null`.
      if (raw !== null && raw !== undefined) {
        throw new CodecError(`unit expected nil, got ${typeofTag(raw)}`);
      }
      return null;
    case "bool":
      if (typeof raw !== "boolean") {
        throw new CodecError(`bool expected, got ${typeofTag(raw)}`);
      }
      return raw;
    case "i8":
    case "i16":
    case "i32":
    case "u8":
    case "u16":
    case "u32": {
      // Range-check on the decode side too: a hostile or buggy peer
      // could send `PaneCreated.cols = 70000` as msgpack uint32 and
      // the JS-side `number` would silently accept it, only to
      // produce impossible values downstream. The encoder enforces
      // the same range via `requireIntLike`; this is the symmetric
      // guard on the wire-in side.
      let coerced: number;
      if (typeof raw === "number") {
        coerced = raw;
      } else if (typeof raw === "bigint") {
        // Small u64 values may come back as bigint with `useBigInt64`;
        // narrow back to number for the int32-and-below slots so the
        // typed JS API uses plain `number`.
        coerced = Number(raw);
      } else {
        throw new CodecError(`${ty.kind} expected number, got ${typeofTag(raw)}`);
      }
      if (!Number.isInteger(coerced)) {
        throw new CodecError(`${ty.kind} got non-integer wire value: ${coerced}`);
      }
      const [min, max] = intRange(ty.kind);
      const bi = BigInt(coerced);
      if (bi < min || bi > max) {
        throw new CodecError(
          `${ty.kind} wire value out of range: ${coerced} (allowed ${min}..=${max})`,
        );
      }
      return coerced;
    }
    case "i64":
    case "i128":
    case "u64":
    case "u128": {
      let bi: bigint;
      if (typeof raw === "bigint") {
        bi = raw;
      } else if (typeof raw === "number") {
        // @msgpack/msgpack falls back to JS number for ints inside the
        // safe range — widen so the typed API uniformly hands out
        // `bigint` for u64-sized fields.
        if (!Number.isInteger(raw)) {
          throw new CodecError(`${ty.kind} got non-integer ${raw}`);
        }
        bi = BigInt(raw);
      } else {
        throw new CodecError(`${ty.kind} expected bigint/number, got ${typeofTag(raw)}`);
      }
      // Range guard: a peer that sends a negative msgpack int into a
      // `u64` slot (or a value past 2^63-1 into `i64`) would silently
      // be accepted as a JS bigint without this check.
      const [min, max] = intRange(ty.kind);
      if (bi < min || bi > max) {
        throw new CodecError(
          `${ty.kind} wire value out of range: ${bi} (allowed ${min}..=${max})`,
        );
      }
      return bi;
    }
    case "f32":
    case "f64":
      if (typeof raw !== "number") {
        throw new CodecError(`${ty.kind} expected number, got ${typeofTag(raw)}`);
      }
      // Symmetric with the encoder: a buggy or hostile peer that
      // sends `NaN` / `Infinity` for a layout-math float (e.g.
      // `ColumnState.widthProportion`, `TileState.weight`) must not
      // pollute downstream computation. Surface the corruption as a
      // CodecError rather than passing it through.
      if (!Number.isFinite(raw)) {
        throw new CodecError(`${ty.kind} wire value is not finite: ${raw}`);
      }
      return raw;
    case "char":
    case "str":
      if (typeof raw !== "string") {
        throw new CodecError(`str expected, got ${typeofTag(raw)}`);
      }
      return raw;
    case "bytes":
      if (raw instanceof Uint8Array) return raw;
      throw new CodecError(`bytes expected Uint8Array, got ${typeofTag(raw)}`);
    case "ref":
      return decodeContainer(raw, requireContainer(ty.name));
    case "option":
      return raw === null || raw === undefined ? null : decodeValue(raw, ty.of);
    case "seq": {
      if (ty.of.kind === "u8") {
        // Wire shape is a msgpack array of fixints; surface as
        // Uint8Array for ergonomic PTY-data handling. Tolerate the
        // unlikely case where the peer uses msgpack `bin` (returned
        // as Uint8Array by @msgpack/msgpack already).
        if (raw instanceof Uint8Array) return raw;
        const arr = requireArray(raw, "seq<u8>");
        const out = new Uint8Array(arr.length);
        for (let i = 0; i < arr.length; i += 1) {
          const elt = arr[i];
          if (typeof elt !== "number") {
            const widened =
              typeof elt === "bigint" ? Number(elt) : Number.NaN;
            if (!Number.isInteger(widened) || widened < 0 || widened > 0xff) {
              throw new CodecError(
                `seq<u8> element ${i} is not a u8: ${typeofTag(elt)}`,
              );
            }
            out[i] = widened;
            continue;
          }
          if (!Number.isInteger(elt) || elt < 0 || elt > 0xff) {
            throw new CodecError(`seq<u8> element ${i} out of u8 range: ${elt}`);
          }
          out[i] = elt;
        }
        return out;
      }
      const arr = requireArray(raw, "seq");
      return arr.map((v) => decodeValue(v, ty.of));
    }
    case "tuple": {
      const arr = requireArray(raw, "tuple");
      if (arr.length !== ty.of.length) {
        throw new CodecError(
          `tuple arity mismatch on wire: schema=${ty.of.length}, got=${arr.length}`,
        );
      }
      return ty.of.map((sub, i) => decodeValue(arr[i], sub));
    }
    case "tuple-array": {
      const arr = requireArray(raw, "tuple-array");
      if (arr.length !== ty.size) {
        throw new CodecError(
          `tuple-array size mismatch: schema=${ty.size}, got=${arr.length}`,
        );
      }
      return arr.map((v) => decodeValue(v, ty.of));
    }
    case "map": {
      // @msgpack/msgpack decodes msgpack maps with string keys to
      // plain JS objects (NOT to `Map` instances), so an `instanceof
      // Map` check would always miss a real `HashMap<String, V>` from
      // the server. Accept both shapes — a JS Map (if the caller's
      // decoder ever opts into one) and a plain object.
      const out = new Map<unknown, unknown>();
      if (raw instanceof Map) {
        for (const [k, v] of raw) {
          out.set(decodeValue(k, ty.key), decodeValue(v, ty.value));
        }
        return out;
      }
      if (raw !== null && typeof raw === "object" && !Array.isArray(raw)) {
        for (const [k, v] of Object.entries(raw as Record<string, unknown>)) {
          out.set(decodeValue(k, ty.key), decodeValue(v, ty.value));
        }
        return out;
      }
      throw new CodecError(`map expected object or Map, got ${typeofTag(raw)}`);
    }
    default:
      throw new CodecError(`unhandled schema kind: ${(ty as { kind: string }).kind}`);
  }
}

// ─── Validation helpers ─────────────────────────────────────────────

function requireContainer(name: string): ContainerSpec {
  const spec = REGISTRY[name];
  if (!spec) {
    throw new CodecError(`no schema entry for container ${JSON.stringify(name)}`);
  }
  return spec;
}

function requireObject(value: unknown, label: string): Record<string, unknown> {
  if (value === null || typeof value !== "object" || Array.isArray(value)) {
    throw new CodecError(`${label} expected object, got ${typeofTag(value)}`);
  }
  return value as Record<string, unknown>;
}

function requireArray(value: unknown, label: string): unknown[] {
  if (!Array.isArray(value)) {
    throw new CodecError(`${label} expected array, got ${typeofTag(value)}`);
  }
  return value;
}

/** Normalise either a JS `number` or `bigint` into something the
 *  `MsgpackWriter` accepts. The caller's `signed` flag drives the
 *  sign check and `ty` drives the upper-bound check. Without the
 *  range check, a value like `MouseInput.button = 300` would encode
 *  as msgpack uint16 and round-trip through the wire only to be
 *  rejected by the server's `rmp_serde::from_slice` while decoding
 *  the `u8` field — collapsing the connection instead of throwing a
 *  local `CodecError`. */
function requireIntLike(
  value: unknown,
  ty: string,
  signed: boolean,
): number | bigint {
  let coerced: number | bigint;
  if (typeof value === "number") {
    if (!Number.isInteger(value)) {
      throw new CodecError(`${ty} expected integer, got ${value}`);
    }
    coerced = value;
  } else if (typeof value === "bigint") {
    coerced = value;
  } else {
    throw new CodecError(`${ty} expected number or bigint, got ${typeofTag(value)}`);
  }
  const [min, max] = intRange(ty);
  if (typeof coerced === "bigint") {
    if (coerced < min || coerced > max) {
      throw new CodecError(`${ty} out of range: ${coerced} (allowed ${min}..=${max})`);
    }
  } else {
    if (BigInt(coerced) < min || BigInt(coerced) > max) {
      throw new CodecError(`${ty} out of range: ${coerced} (allowed ${min}..=${max})`);
    }
  }
  if (!signed) {
    const negative = typeof coerced === "bigint" ? coerced < 0n : coerced < 0;
    if (negative) {
      throw new CodecError(`${ty} expected non-negative integer, got ${coerced}`);
    }
  }
  return coerced;
}

/** Inclusive `[min, max]` for each integer SchemaType kind. */
function intRange(ty: string): [bigint, bigint] {
  switch (ty) {
    case "u8":
      return [0n, 0xffn];
    case "u16":
      return [0n, 0xffffn];
    case "u32":
      return [0n, 0xffffffffn];
    case "u64":
      return [0n, 0xffffffffffffffffn];
    case "u128":
      return [0n, (1n << 128n) - 1n];
    case "i8":
      return [-0x80n, 0x7fn];
    case "i16":
      return [-0x8000n, 0x7fffn];
    case "i32":
      return [-0x80000000n, 0x7fffffffn];
    case "i64":
      return [-(1n << 63n), (1n << 63n) - 1n];
    case "i128":
      return [-(1n << 127n), (1n << 127n) - 1n];
    default:
      // Caller restricts to integer kinds, so anything else is a bug.
      throw new CodecError(`intRange: not an integer type: ${ty}`);
  }
}

function typeofTag(v: unknown): string {
  if (v === null) return "null";
  if (Array.isArray(v)) return "array";
  if (v instanceof Uint8Array) return "Uint8Array";
  if (v instanceof Map) return "Map";
  return typeof v;
}

function describeSchemaType(ty: SchemaType): string {
  switch (ty.kind) {
    case "ref":
      return `ref<${ty.name}>`;
    case "seq":
      return `seq<${describeSchemaType(ty.of)}>`;
    case "option":
      return `option<${describeSchemaType(ty.of)}>`;
    default:
      return ty.kind;
  }
}
