// Bare-URL detection for rendered cell text. Splits a string into
// alternating plain / linkable parts so the renderer can wrap the
// linkable ones in an `<a>`. Mirrors the desktop client's autolink
// (crates/ciri-app/src/grid/link.rs) but scoped to web-openable forms:
// `http(s)://…` and bare `www.…` hosts (promoted to https). File paths
// and other schemes the desktop highlights aren't useful to a browser,
// so they stay plain text. Explicit OSC 8 links are handled separately
// (the whole run is already an `<a>`); this only runs on un-linked runs.

export interface LinkPart {
  text: string;
  /// Resolved href when this part is a clickable URL, else null.
  href: string | null;
}

// An http(s):// URL or a bare `www.` host. Stops at whitespace and the
// few delimiters that can't appear unescaped in a URL. Case-insensitive
// scheme; `g` so we can walk every match on the line.
const URL_RE = /(?:https?:\/\/|www\.)[^\s<>"']+/gi;

// Trailing sentence punctuation that's almost never part of the URL,
// e.g. "see https://x.com." or "https://x.com,". Stripped back out of
// the match and rendered as plain text after the link.
const TRAILING = /[.,;:!?]+$/;

/// Partition `text` into plain + linkable parts. Concatenating every
/// `part.text` reproduces `text` exactly.
export function linkify(text: string): LinkPart[] {
  // Cheap reject: most terminal text has no URL, and running the regex
  // on every row render would be wasteful. Case-insensitive on `www.`
  // to match `URL_RE` (a line of `WWW.EXAMPLE.COM` must not slip past).
  if (!text.includes("://") && !/www\./i.test(text)) {
    return [{ text, href: null }];
  }
  const parts: LinkPart[] = [];
  let last = 0;
  URL_RE.lastIndex = 0;
  for (let m = URL_RE.exec(text); m !== null; m = URL_RE.exec(text)) {
    const start = m.index;
    // Peel trailing junk to a fixed point: sentence punctuation
    // (`.,;:!?`) and a lone trailing ")" (prose parens like
    // "(https://x.com)"). Looping handles interleavings such as
    // "https://x.com.)" → both the ")" and the "." come back out;
    // a Wikipedia-style "…_(macOS)" keeps its balanced paren.
    let url = m[0];
    for (;;) {
      let next = url.replace(TRAILING, "");
      if (next.endsWith(")") && !next.includes("(")) next = next.slice(0, -1);
      if (next === url) break;
      url = next;
    }
    if (url.length === 0) {
      URL_RE.lastIndex = start + m[0].length;
      continue;
    }
    if (start > last) parts.push({ text: text.slice(last, start), href: null });
    const href = /^www\./i.test(url) ? `https://${url}` : url;
    parts.push({ text: url, href });
    last = start + url.length;
    // Re-anchor the scan past the (possibly trimmed) URL so trailing
    // punctuation isn't swallowed or re-examined.
    URL_RE.lastIndex = last;
  }
  if (last < text.length) parts.push({ text: text.slice(last), href: null });
  return parts;
}
