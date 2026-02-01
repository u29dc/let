/**
 * Text sanitization and parsing utilities
 *
 * Transforms raw HTML content into clean, readable text:
 * - Converts <br> to newlines, </p> to double newlines
 * - Strips all HTML tags
 * - Decodes HTML entities
 * - Normalizes whitespace
 * - Parses prices and dates
 */

// =============================================================================
// HTML ENTITY DECODING
// =============================================================================

/** HTML entity mappings */
const HTML_ENTITIES: Record<string, string> = {
	'&amp;': '&',
	'&lt;': '<',
	'&gt;': '>',
	'&quot;': '"',
	'&#39;': "'",
	'&apos;': "'",
	'&nbsp;': ' ',
	'&pound;': '\u00A3',
	'&copy;': '\u00A9',
	'&reg;': '\u00AE',
	'&trade;': '\u2122',
	'&euro;': '\u20AC',
};

/**
 * Decode HTML entities in a string
 * Handles named entities (&amp;) and numeric entities (&#39;)
 */
export function decodeHtmlEntities(html: string): string {
	let result = html;

	for (const [entity, char] of Object.entries(HTML_ENTITIES)) {
		result = result.replaceAll(entity, char);
	}

	result = result.replace(/&#(\d+);/g, (_, code: string) => {
		const num = Number.parseInt(code, 10);
		return String.fromCharCode(num);
	});

	result = result.replace(/&#x([0-9a-fA-F]+);/g, (_, code: string) => {
		const num = Number.parseInt(code, 16);
		return String.fromCharCode(num);
	});

	return result;
}

// =============================================================================
// LINE BREAK CONVERSION
// =============================================================================

/**
 * Convert HTML line breaks to text newlines
 * - <br>, <br/>, <br /> -> single newline
 * - </p> -> double newline (paragraph break)
 * - </li> -> newline (list items)
 */
export function convertLineBreaks(html: string): string {
	return html
		.replace(/<br\s*\/?>/gi, '\n')
		.replace(/<\/p>/gi, '\n\n')
		.replace(/<\/li>/gi, '\n')
		.replace(/<\/div>/gi, '\n');
}

// =============================================================================
// TAG STRIPPING
// =============================================================================

/**
 * Strip all HTML tags from a string
 */
export function stripHtmlTags(html: string): string {
	return html.replace(/<[^>]*>/g, '');
}

// =============================================================================
// WHITESPACE NORMALIZATION
// =============================================================================

/**
 * Normalize whitespace in text
 * - Collapse multiple spaces to single space
 * - Collapse 3+ newlines to double newline
 * - Trim leading/trailing whitespace from each line
 * - Trim overall string
 */
export function normalizeWhitespace(text: string): string {
	return text
		.replace(/[^\S\n]+/g, ' ')
		.split('\n')
		.map((line) => line.trim())
		.join('\n')
		.replace(/\n{3,}/g, '\n\n')
		.trim();
}

// =============================================================================
// SANITIZATION PIPELINES
// =============================================================================

/**
 * Full HTML sanitization pipeline for property descriptions
 *
 * @param html - Raw HTML string from Rightmove
 * @returns Clean, readable plain text
 *
 * @example
 * sanitizeHtml('<p>Spacious 2-bed flat</p><br/>Garden &amp; parking')
 * // => 'Spacious 2-bed flat\n\nGarden & parking'
 */
export function sanitizeHtml(html: string): string {
	if (!html) return '';

	let result = html;
	result = convertLineBreaks(result);
	result = stripHtmlTags(result);
	result = decodeHtmlEntities(result);
	result = normalizeWhitespace(result);

	return result;
}

/**
 * Lightweight sanitization for AI context
 *
 * Combines and normalizes text for machine processing:
 * - Lowercase everything
 * - Strip HTML tags
 * - Decode HTML entities
 * - Collapse whitespace to single spaces
 * - Remove special characters except basic punctuation
 *
 * @param texts - Text strings to combine
 * @returns Single lowercase string for AI context
 */
export function sanitizeForAi(...texts: string[]): string {
	let result = texts.join(' ');

	result = stripHtmlTags(result);
	result = decodeHtmlEntities(result);
	result = result.toLowerCase();
	result = result.replace(/[^\w\s.,!?'-]/g, ' ');
	result = result.replace(/\s+/g, ' ').trim();

	return result;
}

// =============================================================================
// PRICE PARSING
// =============================================================================

/** Weekly price patterns */
const WEEKLY_PATTERNS = [/\bpw\b/i, /per\s*week/i, /p\/w/i, /\bweekly\b/i];
const MONTHLY_PATTERNS = [/\bpcm\b/i, /per\s*(?:calendar\s*)?month/i, /p\/m/i, /\bmonthly\b/i];
const MONTHLY_VALUE_PATTERN = /([\d,.]+)\s*(?:pcm|per\s*(?:calendar\s*)?month|p\/m|monthly)/i;

/**
 * Parse price from Rightmove format
 * "£1,000 pcm" -> 1000
 * "£950 pw" -> 4117 (weekly to monthly)
 * Also handles: "per week", "p/w", "weekly"
 */
export function parsePrice(priceStr: string): number | undefined {
	if (!priceStr) return undefined;

	const normalized = priceStr.toLowerCase();
	const hasMonthly = MONTHLY_PATTERNS.some((pattern) => pattern.test(normalized));
	const hasWeekly = WEEKLY_PATTERNS.some((pattern) => pattern.test(normalized));

	if (hasMonthly && hasWeekly) {
		const monthlyMatch = normalized.match(MONTHLY_VALUE_PATTERN);
		if (monthlyMatch?.[1]) {
			const monthlyAmount = Number.parseFloat(monthlyMatch[1].replace(/,/g, ''));
			if (!Number.isNaN(monthlyAmount)) {
				return Math.round(monthlyAmount);
			}
		}
	}

	const numMatch = normalized.replace(/[£,]/g, '').match(/[\d.]+/);
	if (!numMatch) return undefined;

	const amount = Number.parseFloat(numMatch[0]);
	if (Number.isNaN(amount)) return undefined;

	if (hasWeekly && !hasMonthly) {
		return Math.round(amount * (52 / 12));
	}

	return Math.round(amount);
}

// =============================================================================
// DATE PARSING
// =============================================================================

/** Month name to number mapping */
const MONTH_NAMES: Record<string, string> = {
	january: '01',
	february: '02',
	march: '03',
	april: '04',
	may: '05',
	june: '06',
	july: '07',
	august: '08',
	september: '09',
	october: '10',
	november: '11',
	december: '12',
};

/**
 * Parse date from Rightmove listing history
 * "Added on 15/01/2024" -> "2024-01-15"
 * "Added on 15 January 2024" -> "2024-01-15"
 */
export function parseListedDate(updateReason: string): string | undefined {
	if (!updateReason) return undefined;

	// DD/MM/YYYY format
	const slashMatch = updateReason.match(/(\d{2})\/(\d{2})\/(\d{4})/);
	if (slashMatch) {
		const [, day, month, year] = slashMatch;
		return `${year}-${month}-${day}`;
	}

	// "15 January 2024" format
	const textMatch = updateReason.match(/(\d{1,2})\s+(January|February|March|April|May|June|July|August|September|October|November|December)\s+(\d{4})/i);
	if (textMatch) {
		const [, day, monthName, year] = textMatch;
		const monthNum = MONTH_NAMES[monthName?.toLowerCase() ?? ''];
		if (monthNum && day && year) {
			return `${year}-${monthNum}-${day.padStart(2, '0')}`;
		}
	}

	return undefined;
}
