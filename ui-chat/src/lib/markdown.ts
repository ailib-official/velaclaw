//! GFM Markdown → sanitized HTML for chat assistant bubbles.

import { marked } from "marked";
import DOMPurify from "isomorphic-dompurify";

marked.setOptions({
  gfm: true,
  breaks: true,
});

/**
 * Marked's GFM table tokenizer keeps consuming following lines (even without
 * leading `|`) until a blank line. LLM replies often omit that blank line, so
 * the first paragraph after a table gets sucked into a fake row.
 *
 * Insert a blank line when leaving a pipe-table block.
 */
export function ensureBlankLineAfterTables(src: string): string {
  const lines = src.split("\n");
  const out: string[] = [];
  let inTable = false;
  for (const line of lines) {
    const looksLikeTableRow = /^\s*\|/.test(line);
    if (looksLikeTableRow) {
      inTable = true;
      out.push(line);
      continue;
    }
    if (inTable) {
      inTable = false;
      if (line.trim() !== "") {
        out.push("");
      }
    }
    out.push(line);
  }
  return out.join("\n");
}

/**
 * Render assistant Markdown (GFM: tables, lists, headings, fences) to safe HTML.
 * Empty / whitespace-only input yields an empty string.
 */
export function renderMarkdown(text: string): string {
  const src = text ?? "";
  if (!src.trim()) {
    return "";
  }
  const normalized = ensureBlankLineAfterTables(src);
  const raw = marked.parse(normalized, { async: false }) as string;
  return DOMPurify.sanitize(raw, {
    USE_PROFILES: { html: true },
    ADD_ATTR: ["target", "rel"],
  });
}
