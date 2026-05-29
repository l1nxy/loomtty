// Shared hex helper for the fixture-driven tests. Centralised so both
// the SM and the LZ4 suites use one validated implementation rather
// than a copy-pasted variant per file.

const HEX_PAIR = /^[0-9a-fA-F]{2}$/;

export function hexToBytes(hex: string): Uint8Array {
  if (hex.length % 2 !== 0) {
    throw new Error(`hex string has odd length (${hex.length}); cannot decode`);
  }
  const out = new Uint8Array(hex.length / 2);
  for (let i = 0; i < out.length; i += 1) {
    const pair = hex.slice(i * 2, i * 2 + 2);
    // `parseInt` accepts `"0x"`, leading whitespace, and stops at the
    // first non-hex char — silently producing wrong bytes (e.g.
    // `parseInt("0g", 16) === 0`). A strict regex check before parse
    // guarantees both characters are valid hex digits.
    if (!HEX_PAIR.test(pair)) {
      throw new Error(`invalid hex at byte ${i}: ${JSON.stringify(pair)}`);
    }
    out[i] = parseInt(pair, 16);
  }
  return out;
}
