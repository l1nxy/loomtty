// Schema-driven msgpack codec for ciritty control messages.
//
// rmp_serde's compact externally-tagged enum layout (see the wire-format
// probe in `crates/ciri-protocol/examples/dump_messages.rs`):
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
  // variant name, body in the schema's container shape.
  w.writeMapHeader(1);
  w.writeString(variant.name);
  encodeContainerInto(w, tagged, variant.shape);
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
      w.writeFloat32(value);
      return;
    case "f64":
      if (typeof value !== "number") {
        throw new CodecError(`${ty.kind} expected number, got ${typeofTag(value)}`);
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
      if (typeof raw === "number") return raw;
      if (typeof raw === "bigint") {
        // Small u64 values may come back as bigint with `useBigInt64`;
        // narrow back to number for the int32-and-below slots so the
        // typed JS API uses plain `number`.
        return Number(raw);
      }
      throw new CodecError(`${ty.kind} expected number, got ${typeofTag(raw)}`);
    }
    case "i64":
    case "i128":
    case "u64":
    case "u128": {
      if (typeof raw === "bigint") return raw;
      if (typeof raw === "number") {
        // @msgpack/msgpack falls back to JS number for ints inside the
        // safe range — widen so the typed API uniformly hands out
        // `bigint` for u64-sized fields.
        if (!Number.isInteger(raw)) {
          throw new CodecError(`${ty.kind} got non-integer ${raw}`);
        }
        return BigInt(raw);
      }
      throw new CodecError(`${ty.kind} expected bigint/number, got ${typeofTag(raw)}`);
    }
    case "f32":
    case "f64":
      if (typeof raw !== "number") {
        throw new CodecError(`${ty.kind} expected number, got ${typeofTag(raw)}`);
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
      if (!(raw instanceof Map)) {
        throw new CodecError(`map expected Map, got ${typeofTag(raw)}`);
      }
      const out = new Map<unknown, unknown>();
      for (const [k, v] of raw) {
        out.set(decodeValue(k, ty.key), decodeValue(v, ty.value));
      }
      return out;
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
 *  `MsgpackWriter` accepts for both width-flexible writers. The
 *  caller's `signed` flag drives the sign check; the actual fit-in-N-
 *  bits decision happens inside the writer once the wire form is
 *  chosen, so we don't redo it here. */
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
  if (!signed) {
    const negative = typeof coerced === "bigint" ? coerced < 0n : coerced < 0;
    if (negative) {
      throw new CodecError(`${ty} expected non-negative integer, got ${coerced}`);
    }
  }
  return coerced;
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
