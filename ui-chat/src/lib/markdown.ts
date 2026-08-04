//! GFM Markdown → sanitized HTML for chat assistant bubbles.

import { marked } from "marked";
import DOMPurify from "isomorphic-dompurify";

marked.setOptions({
  gfm: true,
  breaks: true,
});

/**
 * Render assistant Markdown (GFM: tables, lists, headings, fences) to safe HTML.
 * Empty / whitespace-only input yields an empty string.
 */
export function renderMarkdown(text: string): string {
  const src = text ?? "";
  if (!src.trim()) {
    return "";
  }
  const raw = marked.parse(src, { async: false }) as string;
  return DOMPurify.sanitize(raw, {
    USE_PROFILES: { html: true },
    ADD_ATTR: ["target", "rel"],
  });
}
