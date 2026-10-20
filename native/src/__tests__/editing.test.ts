import { describe, it, expect } from 'vitest';
import { change, undo, redo, replaceAll, splitParagraph, mergeParagraph } from '../editing';
import { toVtt } from '../export/vtt';
const source = [{ startMs: 0, endMs: 1000, speaker: 'Alice', text: 'hello world' }];
describe('transcript editing', () => {
  it('retains labels and bounds on replace, split and merge', () => {
    const replaced = replaceAll(source, 'hello', 'hi');
    expect(replaced[0]).toEqual({ ...source[0], text: 'hi world' });
    const split = splitParagraph(source, 0, 5);
    expect(split[0].endMs).toBe(split[1].startMs);
    expect(split.map(s => s.speaker)).toEqual(['Alice', 'Alice']);
    expect(mergeParagraph(split, 0)).toEqual(source);
    expect(mergeParagraph([split[0], { ...split[1], speaker: 'Bob' }], 0)).toHaveLength(2);
    expect(splitParagraph(source, 0, 0)).toBe(source);
  });
  it('undoes/redoes and clears redo when branching', () => {
    const first = { past: [], present: source, future: [] };
    const edited = change(first, replaceAll(source, 'hello', 'hi'));
    expect(undo(edited).present).toEqual(source);
    expect(redo(undo(edited)).present).toEqual(edited.present);
    expect(change(undo(edited), []).future).toEqual([]);
  });
  it('exports VTT with escaped text, labels, and valid cue times', () => {
    expect(toVtt([{ ...source[0], text: '<hello> & world' }])).toContain('00:00:00.000 --> 00:00:01.000\nAlice: &lt;hello&gt; &amp; world');
  });
});
