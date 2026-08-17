// Bare-URL detection for rendered cell text. Splits a string into
// alternating plain / linkable parts so the renderer can wrap the
// linkable ones in an `<a>`. Mirrors the desktop client's autolink
// (crates/loom-app/src/grid/link.rs) but scoped to web-openable forms:
// `http(s)://…` and bare `www.…` hosts (promoted to https). File paths
// and other schemes the desktop highlights aren't useful to a browser,
// so they stay plain text. Explicit OSC 8 links are handled separately
// (the whole run is already an `<a>`); this only runs on un-linked runs.
//
// The scanner is a port of the desktop rules (themselves modelled on VS
// Code's linkComputer): find a scheme, then extend forward until a
// character that can't continue the URL. Brackets are tracked while
// extending — a `)` / `]` / `}` with no matching opener *inside* the URL
// ends it, so markdown `[t](https://…)` and prose `(see https://…)` come
// out clean while `…/Rust_(language)` keeps its balanced pair. Quotes end
// the URL unless it is wrapped in a *different* quote; `*` / `|` end it
// when the URL was introduced by that character. Wide (East Asian) text
// and any non-ASCII punctuation always terminate, so `详见https://x.com。`
// links just the URL.

export interface LinkPart {
  text: string;
  /// Resolved href when this part is a clickable URL, else null.
  href: string | null;
}

// Cheap reject markers — most terminal text has no URL, and running the
// scanner on every row render would be wasteful.
const HAS_SCHEME = /:\/\/|www\./i;

/// Trailing sentence punctuation that's almost never part of the URL.
const TRAILING = new Set([".", ",", ";", ":", "!", "?"]);

/// Length of a supported scheme starting at `i`, or 0.
function schemeLenAt(text: string, i: number): number {
  const rest = text.slice(i, i + 8).toLowerCase();
  if (rest.startsWith("https://")) return 8;
  if (rest.startsWith("http://")) return 7;
  if (rest.startsWith("www.")) {
    // `www.` must start a token-ish thing: not `foo.www.` / `awww.`.
    const prev = i > 0 ? text.charAt(i - 1) : "";
    if (prev === "" || !(isAlnum(prev) || prev === ".")) return 4;
  }
  return 0;
}

function isAlnum(ch: string): boolean {
  return /[\p{L}\p{N}]/u.test(ch);
}

/// East-Asian-wide code point (CJK ideographs, kana, hangul, fullwidth
/// forms, emoji). Approximates unicode-width's `width() >= 2` with the
/// ranges that matter for terminal prose.
function isWide(cp: number): boolean {
  return (
    (cp >= 0x1100 && cp <= 0x115f) ||
    (cp >= 0x2e80 && cp <= 0x303e) ||
    (cp >= 0x3041 && cp <= 0x33ff) ||
    (cp >= 0x3400 && cp <= 0x4dbf) ||
    (cp >= 0x4e00 && cp <= 0x9fff) ||
    (cp >= 0xa000 && cp <= 0xa4cf) ||
    (cp >= 0xac00 && cp <= 0xd7a3) ||
    (cp >= 0xf900 && cp <= 0xfaff) ||
    (cp >= 0xfe30 && cp <= 0xfe4f) ||
    (cp >= 0xff00 && cp <= 0xff60) ||
    (cp >= 0xffe0 && cp <= 0xffe6) ||
    (cp >= 0x1f300 && cp <= 0x1faff) ||
    (cp >= 0x20000 && cp <= 0x3fffd)
  );
}

/// True when the (non-ASCII) character `ch` ends a URL: anything that
/// isn't a letter/digit (typographic quotes, dashes, arrows, fullwidth
/// punctuation, nbsp…) or anything East-Asian-wide.
function terminatesNonAscii(ch: string): boolean {
  return !isAlnum(ch) || isWide(ch.codePointAt(0) ?? 0);
}

/// Extend a URL whose scheme starts at `start` and return the exclusive
/// end index of the URL body, or -1 when nothing usable follows.
function urlEnd(text: string, start: number, schemeLen: number): number {
  const intro = start > 0 ? text.charAt(start - 1) : "";
  let parens = 0;
  let brackets = 0;
  let braces = 0;
  let j = start + schemeLen;
  for (; j < text.length; ) {
    const cp = text.codePointAt(j) as number;
    const ch = String.fromCodePoint(cp);
    let terminate = false;
    switch (ch) {
      case "(":
        parens++;
        break;
      case ")":
        if (parens > 0) parens--;
        else terminate = true;
        break;
      case "[":
        brackets++;
        break;
      case "]":
        if (brackets > 0) brackets--;
        else terminate = true;
        break;
      case "{":
        braces++;
        break;
      case "}":
        if (braces > 0) braces--;
        else terminate = true;
        break;
      case "'":
      case '"':
      case "`":
        // A quote ends the URL unless the URL is wrapped in a *different*
        // quote character (`"https://x/?q='a'"`).
        if (intro === ch) terminate = true;
        else if (intro === "'" || intro === '"' || intro === "`") terminate = false;
        else terminate = true;
        break;
      case "*":
      case "|":
        terminate = intro === ch;
        break;
      case "<":
      case ">":
      case "\\":
      case "^":
        terminate = true;
        break;
      default:
        if (cp <= 0x20 || cp === 0x7f || /\s/u.test(ch)) terminate = true;
        else if (cp > 0x7f && terminatesNonAscii(ch)) terminate = true;
    }
    if (terminate) break;
    j += ch.length;
  }
  const bodyStart = start + schemeLen;
  let end = j;
  while (end > bodyStart && TRAILING.has(text.charAt(end - 1))) end--;
  if (!/[\p{L}\p{N}]/u.test(text.slice(bodyStart, end))) return -1;
  return end;
}

/// Partition `text` into plain + linkable parts. Concatenating every
/// `part.text` reproduces `text` exactly.
export function linkify(text: string): LinkPart[] {
  if (!HAS_SCHEME.test(text)) {
    return [{ text, href: null }];
  }
  const parts: LinkPart[] = [];
  let last = 0;
  let i = 0;
  while (i < text.length) {
    const schemeLen = schemeLenAt(text, i);
    if (schemeLen === 0) {
      i++;
      continue;
    }
    const end = urlEnd(text, i, schemeLen);
    if (end < 0) {
      i += schemeLen;
      continue;
    }
    if (i > last) parts.push({ text: text.slice(last, i), href: null });
    const url = text.slice(i, end);
    const href = /^www\./i.test(url) ? `https://${url}` : url;
    parts.push({ text: url, href });
    last = end;
    i = end;
  }
  if (last < text.length) parts.push({ text: text.slice(last), href: null });
  return parts;
}
