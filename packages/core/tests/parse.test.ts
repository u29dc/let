import { describe, expect, test } from 'bun:test';
import { extractNextData, extractPageModel, findJsonEnd, getPath, isArray, isNumber, isObject, isString } from '@let/core/pipeline/parse';

describe('findJsonEnd', () => {
	test('finds end of simple object', () => {
		const json = '{"a":1}';
		expect(findJsonEnd(json)).toBe(6);
	});

	test('finds end of nested object', () => {
		const json = '{"a":{"b":{"c":1}}}rest';
		expect(findJsonEnd(json)).toBe(18);
	});

	test('handles strings with braces', () => {
		const json = '{"text":"hello { world }"}';
		expect(findJsonEnd(json)).toBe(25);
	});

	test('handles escaped quotes', () => {
		const json = '{"text":"say \\"hi\\""}';
		expect(findJsonEnd(json)).toBe(20);
	});

	test('returns -1 for non-object', () => {
		expect(findJsonEnd('hello')).toBe(-1);
		expect(findJsonEnd('[1,2,3]')).toBe(-1);
	});

	test('returns -1 for unclosed object', () => {
		expect(findJsonEnd('{"a":1')).toBe(-1);
	});
});

describe('extractPageModel', () => {
	test('extracts PAGE_MODEL from valid HTML', () => {
		const html = `
      <html>
      <script>window.PAGE_MODEL = {"propertyData":{"id":123}}</script>
      </html>
    `;
		const result = extractPageModel(html);

		expect(result.success).toBe(true);
		if (result.success) {
			expect(result.data).toEqual({ propertyData: { id: 123 } });
		}
	});

	test('handles nested JSON', () => {
		const html = `
      <script>window.PAGE_MODEL = {"a":{"b":{"c":[1,2,3]}}}</script>
    `;
		const result = extractPageModel(html);

		expect(result.success).toBe(true);
		if (result.success) {
			expect(result.data).toEqual({ a: { b: { c: [1, 2, 3] } } });
		}
	});

	test('returns error when marker not found', () => {
		const html = '<html><body>No model here</body></html>';
		const result = extractPageModel(html);

		expect(result.success).toBe(false);
		if (!result.success) {
			expect(result.error).toContain('not found');
		}
	});

	test('returns error for invalid JSON', () => {
		const html = '<script>window.PAGE_MODEL = {invalid json}</script>';
		const result = extractPageModel(html);

		expect(result.success).toBe(false);
		if (!result.success) {
			expect(result.error).toContain('Invalid JSON');
		}
	});
});

describe('extractNextData', () => {
	test('extracts __NEXT_DATA__ from valid HTML', () => {
		const html = `
      <html>
      <script id="__NEXT_DATA__" type="application/json">
        {"props":{"pageProps":{"data":"test"}}}
      </script>
      </html>
    `;
		const result = extractNextData(html);

		expect(result.success).toBe(true);
		if (result.success) {
			expect(result.data).toEqual({ props: { pageProps: { data: 'test' } } });
		}
	});

	test('returns error when script not found', () => {
		const html = '<html><body>No next data</body></html>';
		const result = extractNextData(html);

		expect(result.success).toBe(false);
		if (!result.success) {
			expect(result.error).toContain('not found');
		}
	});
});

describe('getPath', () => {
	test('gets shallow property', () => {
		expect(getPath({ a: 1 }, 'a')).toBe(1);
	});

	test('gets nested property', () => {
		expect(getPath({ a: { b: { c: 'deep' } } }, 'a.b.c')).toBe('deep');
	});

	test('returns undefined for missing path', () => {
		expect(getPath({ a: 1 }, 'b')).toBeUndefined();
		expect(getPath({ a: 1 }, 'a.b.c')).toBeUndefined();
	});

	test('handles null values', () => {
		expect(getPath({ a: null }, 'a.b')).toBeUndefined();
	});

	test('handles arrays in path', () => {
		expect(getPath({ a: [1, 2, 3] }, 'a.0')).toBe(1);
	});
});

describe('type guards', () => {
	test('isObject', () => {
		expect(isObject({})).toBe(true);
		expect(isObject({ a: 1 })).toBe(true);
		expect(isObject(null)).toBe(false);
		expect(isObject([])).toBe(false);
		expect(isObject('string')).toBe(false);
	});

	test('isArray', () => {
		expect(isArray([])).toBe(true);
		expect(isArray([1, 2, 3])).toBe(true);
		expect(isArray({})).toBe(false);
		expect(isArray(null)).toBe(false);
	});

	test('isString', () => {
		expect(isString('')).toBe(true);
		expect(isString('hello')).toBe(true);
		expect(isString(123)).toBe(false);
		expect(isString(null)).toBe(false);
	});

	test('isNumber', () => {
		expect(isNumber(0)).toBe(true);
		expect(isNumber(123)).toBe(true);
		expect(isNumber(-1.5)).toBe(true);
		expect(isNumber(Number.NaN)).toBe(false);
		expect(isNumber('123')).toBe(false);
	});
});
