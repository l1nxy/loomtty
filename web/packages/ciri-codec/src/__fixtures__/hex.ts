// Shared hex helper for the fixture-driven tests. Centralised so both
// the SM and the LZ4 suites use one validated implementation rather
// than a copy-pasted variant per file.

export function hexToBytes(hex: string): Uint8Array {
  if (hex.length % 2 !== 0) {
    throw new Error(`hex string has odd length (${hex.length}); cannot decode`);
  }
  const out = new Uint8Array(hex.length / 2);
  for (let i = 0; i < out.length; i += 1) {
    const byte = parseInt(hex.substr(i * 2, 2), 16);
    if (Number.isNaN(byte)) {
      throw new Error(
        `invalid hex at byte ${i}: ${JSON.stringify(hex.substr(i * 2, 2))}`,
      );
    }
    out[i] = byte;
  }
  return out;
}
