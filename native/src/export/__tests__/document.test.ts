import { expect, it } from 'vitest';
import { exportDocument } from '../document';

const segments = [{ startMs: 1234, endMs: 3200, speaker: 'Олена <A>', text: 'Привіт; "world"\n<script>alert(1)</script>' }];

it('exports structured JSON with exact timestamp and speaker data', () => {
  expect(JSON.parse(exportDocument('Interview', segments, 'json'))).toEqual({ version: 1, title: 'Interview', timestampUnit: 'milliseconds', segments });
});

it('exports standalone HTML without executable transcript markup or remote assets', () => {
  const result = exportDocument('<img src=x onerror=alert(1)>', segments, 'html');
  expect(result).toContain('&lt;script&gt;alert(1)&lt;/script&gt;');
  expect(result).toContain('Олена &lt;A&gt;');
  expect(result).toContain('00:00:01.234 – 00:00:03.200');
  expect(result).toContain("default-src 'none'");
  expect(result).not.toMatch(/<script|<img|<[^>]+\ssrc=/);
});

it('quotes spreadsheet cells, retains Unicode and neutralizes formulas', () => {
  const result = exportDocument('Ignored', [...segments, { ...segments[0], speaker: '=HYPERLINK("url")', text: ' \t+SUM(1,2)' }], 'csv');
  expect(result).toContain('"Привіт; ""world""\n<script>alert(1)</script>"');
  expect(result).toContain('"\'=HYPERLINK(""url"")"');
  expect(result).toContain('"\' \t+SUM(1,2)"');
  expect(result).toContain('"1234";"3200";"Олена <A>"');
});

it('escapes Markdown title, names and paragraphs while preserving readable text', () => {
  const result = exportDocument('[title](javascript:x)\nheading', segments, 'md');
  expect(result).toContain('# \\[title\\](javascript:x) heading');
  expect(result).toContain('00:00:01.234 – 00:00:03.200');
  expect(result).toContain('Олена \\<A\\>');
  expect(result).toContain('\\<script\\>alert(1)\\</script\\>');
});
