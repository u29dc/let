import { describe, expect, test } from 'bun:test';
import { convertLineBreaks, decodeHtmlEntities, normalizeWhitespace, sanitizeHtml, stripHtmlTags } from '@let/core/pipeline/parse';

describe('decodeHtmlEntities', () => {
	test('decodes named entities', () => {
		expect(decodeHtmlEntities('&amp;')).toBe('&');
		expect(decodeHtmlEntities('&lt;')).toBe('<');
		expect(decodeHtmlEntities('&gt;')).toBe('>');
		expect(decodeHtmlEntities('&quot;')).toBe('"');
		expect(decodeHtmlEntities('&#39;')).toBe("'");
		expect(decodeHtmlEntities('&nbsp;')).toBe(' ');
	});

	test('decodes numeric entities (decimal)', () => {
		expect(decodeHtmlEntities('&#65;')).toBe('A');
		expect(decodeHtmlEntities('&#163;')).toBe('£');
	});

	test('decodes numeric entities (hex)', () => {
		expect(decodeHtmlEntities('&#x41;')).toBe('A');
		expect(decodeHtmlEntities('&#xA3;')).toBe('£');
	});

	test('decodes multiple entities in string', () => {
		expect(decodeHtmlEntities('Tom &amp; Jerry &lt;3')).toBe('Tom & Jerry <3');
	});

	test('leaves non-entities unchanged', () => {
		expect(decodeHtmlEntities('Hello world')).toBe('Hello world');
	});
});

describe('convertLineBreaks', () => {
	test('converts <br> to newline', () => {
		expect(convertLineBreaks('line1<br>line2')).toBe('line1\nline2');
		expect(convertLineBreaks('line1<br/>line2')).toBe('line1\nline2');
		expect(convertLineBreaks('line1<br />line2')).toBe('line1\nline2');
	});

	test('converts </p> to double newline', () => {
		expect(convertLineBreaks('<p>para1</p><p>para2</p>')).toBe('<p>para1\n\n<p>para2\n\n');
	});

	test('converts </div> to newline', () => {
		expect(convertLineBreaks('<div>block</div>')).toBe('<div>block\n');
	});
});

describe('stripHtmlTags', () => {
	test('strips simple tags', () => {
		expect(stripHtmlTags('<p>text</p>')).toBe('text');
		expect(stripHtmlTags('<strong>bold</strong>')).toBe('bold');
	});

	test('strips tags with attributes', () => {
		expect(stripHtmlTags('<a href="url">link</a>')).toBe('link');
		expect(stripHtmlTags('<div class="foo" id="bar">content</div>')).toBe('content');
	});

	test('strips self-closing tags', () => {
		expect(stripHtmlTags('before<img src="x"/>after')).toBe('beforeafter');
	});

	test('handles nested tags', () => {
		expect(stripHtmlTags('<div><p><strong>nested</strong></p></div>')).toBe('nested');
	});
});

describe('normalizeWhitespace', () => {
	test('collapses multiple spaces', () => {
		expect(normalizeWhitespace('hello    world')).toBe('hello world');
	});

	test('trims lines', () => {
		expect(normalizeWhitespace('  hello  \n  world  ')).toBe('hello\nworld');
	});

	test('collapses 3+ newlines to double', () => {
		expect(normalizeWhitespace('a\n\n\n\nb')).toBe('a\n\nb');
	});

	test('preserves double newlines', () => {
		expect(normalizeWhitespace('a\n\nb')).toBe('a\n\nb');
	});

	test('trims overall string', () => {
		expect(normalizeWhitespace('  \n  text  \n  ')).toBe('text');
	});
});

describe('sanitizeHtml', () => {
	test('full pipeline - simple case', () => {
		const input = '<p>Spacious 2-bed flat</p><br/>Garden &amp; parking';
		const expected = 'Spacious 2-bed flat\n\nGarden & parking';
		expect(sanitizeHtml(input)).toBe(expected);
	});

	test('full pipeline - complex description', () => {
		const input = `
      <p>Beautiful property with <strong>private garden</strong>.</p>
      <p>Features include:</p>
      <ul>
        <li>Gas central heating</li>
        <li>Double glazing</li>
      </ul>
      <p>EPC Rating: B</p>
    `;
		const result = sanitizeHtml(input);

		expect(result).toContain('Beautiful property');
		expect(result).toContain('private garden');
		expect(result).toContain('Gas central heating');
		expect(result).toContain('EPC Rating: B');
		expect(result).not.toContain('<');
		expect(result).not.toContain('>');
	});

	test('handles empty input', () => {
		expect(sanitizeHtml('')).toBe('');
	});

	test('handles plain text', () => {
		expect(sanitizeHtml('Just plain text')).toBe('Just plain text');
	});
});
