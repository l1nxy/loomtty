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

  test("does not linkify unsupported schemes", () => {
    // No "://" and no "www." marker → cheap-reject path, plain text.
    expect(linkify("javascript:alert(1)")).toEqual([
      { text: "javascript:alert(1)", href: null },
    ]);
  });
});
