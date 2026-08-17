import { describe, expect, test } from "vitest";
import { linkify } from "./linkify.js";

describe("linkify", () => {
  test("plain text with no URL is a single non-link part", () => {
    expect(linkify("just some text")).toEqual([
      { text: "just some text", href: null },
    ]);
  });

  test("splits an https URL out of surrounding prose", () => {
    expect(linkify("see https://example.com here")).toEqual([
      { text: "see ", href: null },
      { text: "https://example.com", href: "https://example.com" },
      { text: " here", href: null },
    ]);
  });

  test("promotes a bare www. host to https", () => {
    expect(linkify("go www.example.com")).toEqual([
      { text: "go ", href: null },
      { text: "www.example.com", href: "https://www.example.com" },
    ]);
  });

  test("trims trailing sentence punctuation out of the link", () => {
    expect(linkify("read https://example.com/path.")).toEqual([
      { text: "read ", href: null },
      {
        text: "https://example.com/path",
        href: "https://example.com/path",
      },
      { text: ".", href: null },
    ]);
  });

  test("drops a lone wrapping close-paren but keeps balanced ones", () => {
    expect(linkify("(https://example.com)")).toEqual([
      { text: "(", href: null },
      { text: "https://example.com", href: "https://example.com" },
      { text: ")", href: null },
    ]);
    const wiki = linkify("https://en.wikipedia.org/wiki/Terminal_(macOS)");
    expect(wiki).toEqual([
      {
        text: "https://en.wikipedia.org/wiki/Terminal_(macOS)",
        href: "https://en.wikipedia.org/wiki/Terminal_(macOS)",
      },
    ]);
  });

  test("uppercase WWW. is still linkified (case-insensitive cheap reject)", () => {
    expect(linkify("visit WWW.Example.COM now")).toEqual([
      { text: "visit ", href: null },
      { text: "WWW.Example.COM", href: "https://WWW.Example.COM" },
      { text: " now", href: null },
    ]);
  });

  test("strips punctuation that precedes a wrapping close-paren", () => {
    expect(linkify("(https://example.com.)")).toEqual([
      { text: "(", href: null },
      { text: "https://example.com", href: "https://example.com" },
      { text: ".)", href: null },
    ]);
  });

  test("handles two URLs on one line", () => {
    const parts = linkify("a http://x.io b https://y.io c");
    expect(parts.map((p) => p.href)).toEqual([
      null,
      "http://x.io",
      null,
      "https://y.io",
      null,
    ]);
    // Round-trips the original text.
    expect(parts.map((p) => p.text).join("")).toBe("a http://x.io b https://y.io c");
  });

  test("markdown link: only the URL inside the parens is linked", () => {
    expect(linkify("[docs](https://example.com/guide)")).toEqual([
      { text: "[docs](", href: null },
      { text: "https://example.com/guide", href: "https://example.com/guide" },
      { text: ")", href: null },
    ]);
  });

  test("markdown autolink: the `](` between two URLs belongs to neither", () => {
    const parts = linkify("[https://a.io](https://a.io)");
    expect(parts).toEqual([
      { text: "[", href: null },
      { text: "https://a.io", href: "https://a.io" },
      { text: "](", href: null },
      { text: "https://a.io", href: "https://a.io" },
      { text: ")", href: null },
    ]);
  });

  test("CJK prose and fullwidth punctuation glued to a URL stay outside it", () => {
    expect(linkify("请访问https://example.com查看")).toEqual([
      { text: "请访问", href: null },
      { text: "https://example.com", href: "https://example.com" },
      { text: "查看", href: null },
    ]);
    expect(linkify("详见https://example.com/foo，谢谢")).toEqual([
      { text: "详见", href: null },
      { text: "https://example.com/foo", href: "https://example.com/foo" },
      { text: "，谢谢", href: null },
    ]);
    expect(linkify("（https://example.com/path）。")).toEqual([
      { text: "（", href: null },
      { text: "https://example.com/path", href: "https://example.com/path" },
      { text: "）。", href: null },
    ]);
  });

  test("markdown emphasis / code fences around a URL", () => {
    expect(linkify("**https://example.com/a**")).toEqual([
      { text: "**", href: null },
      { text: "https://example.com/a", href: "https://example.com/a" },
      { text: "**", href: null },
    ]);
    expect(linkify("`https://example.com/a`")).toEqual([
      { text: "`", href: null },
      { text: "https://example.com/a", href: "https://example.com/a" },
      { text: "`", href: null },
    ]);
  });

  test("quotes end a URL unless it is wrapped in a different quote", () => {
    expect(linkify("\"url\":\"https://example.com/\"")).toEqual([
      { text: '"url":"', href: null },
      { text: "https://example.com/", href: "https://example.com/" },
      { text: '"', href: null },
    ]);
    expect(linkify("\"https://example.com/a?q='x'\"")).toEqual([
      { text: '"', href: null },
      {
        text: "https://example.com/a?q='x'",
        href: "https://example.com/a?q='x'",
      },
      { text: '"', href: null },
    ]);
  });

  test("key=value prefixes and unbalanced closers", () => {
    expect(linkify("url=https://example.com/x)")).toEqual([
      { text: "url=", href: null },
      { text: "https://example.com/x", href: "https://example.com/x" },
      { text: ")", href: null },
    ]);
    expect(linkify("http://[::1]:8080/x")).toEqual([
      { text: "http://[::1]:8080/x", href: "http://[::1]:8080/x" },
    ]);
  });

  test("www. needs a token boundary; a bare scheme is not a link", () => {
    expect(linkify("wwww.example.com")).toEqual([
      { text: "wwww.example.com", href: null },
    ]);
    expect(linkify("see https:// there")).toEqual([
      { text: "see https:// there", href: null },
    ]);
  });

  test("does not linkify unsupported schemes", () => {
    // No "://" and no "www." marker → cheap-reject path, plain text.
    expect(linkify("javascript:alert(1)")).toEqual([
      { text: "javascript:alert(1)", href: null },
    ]);
  });
});
