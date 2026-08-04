import { describe, expect, it } from "vitest";
import { renderMarkdown } from "./markdown";

describe("renderMarkdown", () => {
  it("renders headings, bold, and paragraphs", () => {
    const html = renderMarkdown("## Title\n\n**bold** text");
    expect(html).toContain("<h2>");
    expect(html).toContain("Title");
    expect(html).toContain("<strong>bold</strong>");
  });

  it("renders GFM tables", () => {
    const md = `| Type | Count |
|------|------:|
| VLESS | 2 |
| VMess | 0 |`;
    const html = renderMarkdown(md);
    expect(html).toContain("<table>");
    expect(html).toContain("<th>");
    expect(html).toContain("VLESS");
    expect(html).toContain("<td>");
  });

  it("renders nested lists", () => {
    const md = `- root
  - child
1. one
2. two`;
    const html = renderMarkdown(md);
    expect(html).toContain("<ul>");
    expect(html).toContain("<ol>");
    expect(html).toContain("<li>");
  });

  it("renders fenced code blocks", () => {
    const html = renderMarkdown("```bash\necho hi\n```");
    expect(html).toContain("<pre>");
    expect(html).toMatch(/<code\b/);
    expect(html).toContain("echo hi");
  });

  it("sanitizes script injection", () => {
    const html = renderMarkdown('hello <script>alert(1)</script> **ok**');
    expect(html.toLowerCase()).not.toContain("<script");
    expect(html).toContain("<strong>ok</strong>");
  });

  it("returns empty for blank input", () => {
    expect(renderMarkdown("")).toBe("");
    expect(renderMarkdown("   \n")).toBe("");
  });

  it("does not absorb following paragraph into a GFM table", () => {
    const md = `| # | 时长 |
|---|------|
| 1 | 78 秒 |
| 5 | 65 秒 |
**规律**：每次开机都很短。
- 凌晨那次`;
    const html = renderMarkdown(md);
    expect(html).toContain("<table>");
    expect(html).toContain("</table>");
    // Paragraph must be outside the table, not an extra <td>.
    expect(html).toMatch(/<\/table>\s*<p>.*规律/);
    expect(html).not.toMatch(/<td><strong>规律<\/strong>/);
    expect(html).toContain("<ul>");
  });
});
