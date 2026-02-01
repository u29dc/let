/**
 * JSON extraction utilities for Rightmove pages
 *
 * Extracts structured JSON data from:
 * - Individual listing pages: window.PAGE_MODEL = {...}
 * - Search results pages: <script id="__NEXT_DATA__">...</script>
 */

/**
 * Result of a parse operation
 */
export type ParseResult<T> = { success: true; data: T } | { success: false; error: string };

/** State for JSON brace counting parser */
type JsonParseState = { depth: number; inString: boolean; escaped: boolean };

/** Process a single character and update parser state, returns closing index or -1 */
function processJsonChar(char: string, index: number, state: JsonParseState): number {
	if (state.escaped) {
		state.escaped = false;
		return -1;
	}
	if (char === '\\' && state.inString) {
		state.escaped = true;
		return -1;
	}
	if (char === '"') {
		state.inString = !state.inString;
		return -1;
	}
	if (state.inString) return -1;
	if (char === '{') state.depth++;
	if (char === '}') {
		state.depth--;
		if (state.depth === 0) return index;
	}
	return -1;
}

/**
 * Find the end of a JSON object using brace counting
 *
 * Simple regex fails for nested JSON - we need to count braces
 * to find where the object actually ends.
 *
 * @param str - String starting with '{'
 * @returns Index of closing brace, or -1 if not found
 */
export function findJsonEnd(str: string): number {
	if (str[0] !== '{') return -1;

	const state: JsonParseState = { depth: 0, inString: false, escaped: false };

	for (let i = 0; i < str.length; i++) {
		const result = processJsonChar(str[i] ?? '', i, state);
		if (result !== -1) return result;
	}

	return -1;
}

/**
 * Extract PAGE_MODEL JSON from a Rightmove listing page
 *
 * Listing pages contain: <script>window.PAGE_MODEL = {...}</script>
 * The JSON is deeply nested, so we use brace-counting to find the end.
 *
 * @param html - Full HTML of a Rightmove listing page
 * @returns Parsed PAGE_MODEL object or error
 */
export function extractPageModel(html: string): ParseResult<unknown> {
	const pattern = /window\.(?:PAGE_MODEL|pageModel)\s*=\s*/;
	const match = pattern.exec(html);

	if (!match) {
		return { success: false, error: 'PAGE_MODEL marker not found' };
	}

	const jsonStart = match.index + match[0].length;
	const remaining = html.slice(jsonStart);

	const jsonEnd = findJsonEnd(remaining);
	if (jsonEnd === -1) {
		return { success: false, error: 'Could not find end of PAGE_MODEL JSON' };
	}

	const jsonStr = remaining.slice(0, jsonEnd + 1);

	try {
		const data: unknown = JSON.parse(jsonStr);
		return { success: true, data };
	} catch (e) {
		const message = e instanceof Error ? e.message : 'Unknown parse error';
		return { success: false, error: `Invalid JSON in PAGE_MODEL: ${message}` };
	}
}

/**
 * Extract __NEXT_DATA__ JSON from a Rightmove search results page
 *
 * Search pages use Next.js and contain:
 * <script id="__NEXT_DATA__" type="application/json">{...}</script>
 *
 * @param html - Full HTML of a Rightmove search results page
 * @returns Parsed __NEXT_DATA__ object or error
 */
export function extractNextData(html: string): ParseResult<unknown> {
	const pattern = /<script[^>]*id=['"]__NEXT_DATA__['"][^>]*>([\s\S]*?)<\/script>/i;
	const match = pattern.exec(html);

	if (!match?.[1]) {
		return { success: false, error: '__NEXT_DATA__ script tag not found' };
	}

	const jsonStr = match[1].trim();

	try {
		const data: unknown = JSON.parse(jsonStr);
		return { success: true, data };
	} catch (e) {
		const message = e instanceof Error ? e.message : 'Unknown parse error';
		return { success: false, error: `Invalid JSON in __NEXT_DATA__: ${message}` };
	}
}

/**
 * Safely access nested properties in an unknown object
 *
 * @param obj - Object to traverse
 * @param path - Dot-separated path (e.g., 'propertyData.location.latitude')
 * @returns Value at path or undefined
 */
export function getPath(obj: unknown, path: string): unknown {
	const parts = path.split('.');
	let current: unknown = obj;

	for (const part of parts) {
		if (current === null || current === undefined) {
			return undefined;
		}
		if (typeof current !== 'object') {
			return undefined;
		}
		current = (current as Record<string, unknown>)[part];
	}

	return current;
}

/**
 * Type guard for checking if value is a non-null object
 */
export function isObject(value: unknown): value is Record<string, unknown> {
	return typeof value === 'object' && value !== null && !Array.isArray(value);
}

/**
 * Type guard for checking if value is an array
 */
export function isArray(value: unknown): value is unknown[] {
	return Array.isArray(value);
}

/**
 * Type guard for string
 */
export function isString(value: unknown): value is string {
	return typeof value === 'string';
}

/**
 * Type guard for number
 */
export function isNumber(value: unknown): value is number {
	return typeof value === 'number' && !Number.isNaN(value);
}
