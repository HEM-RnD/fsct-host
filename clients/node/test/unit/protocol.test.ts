import { describe, it, expect } from 'vitest';
import { MAX_LINE_BYTES, parseNdjsonChunk } from '../../src/protocol.js';

describe('parseNdjsonChunk', () => {
  it('parses a single complete line', () => {
    const { lines, remainder, oversized } = parseNdjsonChunk('', '{"id":1}\n');
    expect(oversized).toBe(false);
    expect(lines).toEqual(['{"id":1}']);
    expect(remainder).toBe('');
  });

  it('parses multiple lines in one chunk', () => {
    const { lines, remainder, oversized } = parseNdjsonChunk('', '{"id":1}\n{"id":2}\n{"id":3}\n');
    expect(oversized).toBe(false);
    expect(lines).toEqual(['{"id":1}', '{"id":2}', '{"id":3}']);
    expect(remainder).toBe('');
  });

  it('accumulates a fragmented line across two chunks', () => {
    const first = parseNdjsonChunk('', '{"id"');
    expect(first.lines).toEqual([]);
    expect(first.remainder).toBe('{"id"');

    const second = parseNdjsonChunk(first.remainder, ':1}\n');
    expect(second.lines).toEqual(['{"id":1}']);
    expect(second.remainder).toBe('');
  });

  it('accumulates across three chunks', () => {
    const r1 = parseNdjsonChunk('', '{"a"');
    const r2 = parseNdjsonChunk(r1.remainder, ':"b"');
    const r3 = parseNdjsonChunk(r2.remainder, '}\n');
    expect(r3.lines).toEqual(['{"a":"b"}']);
    expect(r3.remainder).toBe('');
  });

  it('returns an empty remainder when chunk ends exactly at newline', () => {
    const { lines, remainder } = parseNdjsonChunk('', 'line1\n');
    expect(lines).toEqual(['line1']);
    expect(remainder).toBe('');
  });

  it('keeps partial trailing content in remainder', () => {
    const { lines, remainder } = parseNdjsonChunk('', 'line1\npartial');
    expect(lines).toEqual(['line1']);
    expect(remainder).toBe('partial');
  });

  it('handles empty chunk with existing buffer', () => {
    const { lines, remainder, oversized } = parseNdjsonChunk('partial', '');
    expect(oversized).toBe(false);
    expect(lines).toEqual([]);
    expect(remainder).toBe('partial');
  });

  it('handles completely empty input', () => {
    const { lines, remainder, oversized } = parseNdjsonChunk('', '');
    expect(oversized).toBe(false);
    expect(lines).toEqual([]);
    expect(remainder).toBe('');
  });

  it('returns oversized=false for a line exactly at MAX_LINE_BYTES', () => {
    const line = 'x'.repeat(MAX_LINE_BYTES);
    // Does not include the newline, but combined buffer equals MAX_LINE_BYTES.
    const { oversized } = parseNdjsonChunk('', line);
    expect(oversized).toBe(false);
  });

  it('returns oversized=true when accumulated content exceeds MAX_LINE_BYTES', () => {
    const big = 'x'.repeat(MAX_LINE_BYTES + 1);
    const { oversized } = parseNdjsonChunk('', big);
    expect(oversized).toBe(true);
  });

  it('includes empty lines from double newline', () => {
    const { lines } = parseNdjsonChunk('', 'line1\n\nline2\n');
    expect(lines).toEqual(['line1', '', 'line2']);
  });
});
