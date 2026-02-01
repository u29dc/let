/**
 * Notion API client for exporting listings
 *
 * Syncs property listings to a Notion database with proper types.
 * Uses external file URLs for images (Rightmove CDN).
 */

import type { Listing } from '../../schema/index.js';
import { log } from '../../utils/logger.js';

// =============================================================================
// TYPES
// =============================================================================

export interface NotionConfig {
	apiKey: string;
	databaseId: string;
}

interface NotionError {
	object: 'error';
	status: number;
	code: string;
	message: string;
}

interface NotionPage {
	id: string;
	object: 'page';
	properties: Record<string, unknown>;
}

interface NotionQueryResponse {
	object: 'list';
	results: NotionPage[];
	has_more: boolean;
	next_cursor: string | null;
}

// =============================================================================
// CONSTANTS
// =============================================================================

const NOTION_API_VERSION = '2022-06-28';
const NOTION_BASE_URL = 'https://api.notion.com/v1';

// Rate limiter state
let lastRequestTime = 0;
const MIN_DELAY_MS = 350;
const NOTION_MAX_RETRIES = 3;
const NOTION_RETRY_BASE_MS = 1000;

/**
 * Simple rate limiter for Notion API (3 requests per second)
 */
async function rateLimitDelay(): Promise<void> {
	const now = Date.now();
	const elapsed = now - lastRequestTime;
	if (elapsed < MIN_DELAY_MS) {
		await new Promise((resolve) => setTimeout(resolve, MIN_DELAY_MS - elapsed));
	}
	lastRequestTime = Date.now();
}

function sleep(ms: number): Promise<void> {
	return new Promise((resolve) => setTimeout(resolve, ms));
}

function shouldRetry(status?: number): boolean {
	if (!status) return true;
	return status === 429 || status >= 500;
}

function getRetryDelay(retryAfter: string | null, attempt: number): number {
	if (retryAfter) {
		const seconds = Number.parseInt(retryAfter, 10);
		if (!Number.isNaN(seconds)) return seconds * 1000;
	}
	return NOTION_RETRY_BASE_MS * 2 ** (attempt - 1) + Math.random() * 250;
}

async function parseErrorBody(response: Response): Promise<NotionError | null> {
	try {
		const data = (await response.json()) as NotionError;
		if (data && data.object === 'error') return data;
		return null;
	} catch {
		return null;
	}
}

type NotionAttemptResult<T> = { ok: true; data: T } | { ok: false; retryable: boolean; error: Error; status?: number; retryAfter?: string | null };

function buildRequestInit(config: NotionConfig, options: RequestInit): RequestInit {
	return {
		...options,
		headers: {
			Authorization: `Bearer ${config.apiKey}`,
			'Notion-Version': NOTION_API_VERSION,
			'Content-Type': 'application/json',
			...options.headers,
		},
	};
}

function buildNotionError(message: string, code: string): Error {
	return new Error(`Notion API error: ${message} (${code})`);
}

async function attemptNotionRequest<T>(config: NotionConfig, url: string, options: RequestInit): Promise<NotionAttemptResult<T>> {
	try {
		const response = await fetch(url, buildRequestInit(config, options));
		if (response.ok) {
			const data = (await response.json()) as T;
			return { ok: true, data };
		}

		const errorBody = await parseErrorBody(response);
		const message = errorBody?.message ?? response.statusText ?? 'Unknown error';
		const code = errorBody?.code ?? 'unknown_error';
		const error = buildNotionError(message, code);
		return {
			ok: false,
			retryable: shouldRetry(response.status),
			error,
			status: response.status,
			retryAfter: response.headers.get('Retry-After'),
		};
	} catch (error) {
		const message = error instanceof Error ? error.message : String(error);
		return { ok: false, retryable: true, error: new Error(message) };
	}
}

// =============================================================================
// API HELPERS
// =============================================================================

async function notionFetch<T>(config: NotionConfig, endpoint: string, options: RequestInit = {}): Promise<T> {
	const url = `${NOTION_BASE_URL}${endpoint}`;
	let lastError = 'Unknown error';

	for (let attempt = 1; attempt <= NOTION_MAX_RETRIES; attempt++) {
		await rateLimitDelay();
		const result = await attemptNotionRequest<T>(config, url, options);
		if (result.ok) return result.data;

		lastError = result.error.message;
		if (!result.retryable || attempt === NOTION_MAX_RETRIES) {
			throw result.error;
		}

		const delay = getRetryDelay(result.retryAfter ?? null, attempt);
		log.notion.warn('Notion API retrying', { status: result.status, attempt, delay: Math.round(delay) });
		await sleep(delay);
	}

	throw new Error(lastError);
}

// =============================================================================
// PROPERTY BUILDERS
// =============================================================================

function buildTitle(text: string): { title: Array<{ text: { content: string } }> } {
	return {
		title: [{ text: { content: text.slice(0, 2000) } }],
	};
}

function buildRichText(text: string | null | undefined): { rich_text: Array<{ text: { content: string } }> } {
	return {
		rich_text: text ? [{ text: { content: text.slice(0, 2000) } }] : [],
	};
}

function buildNumber(value: number | null | undefined): { number: number | null } {
	return { number: value ?? null };
}

function buildSelect(value: string | null | undefined): { select: { name: string } | null } {
	return { select: value ? { name: value } : null };
}

function buildUrl(url: string | null | undefined): { url: string | null } {
	return { url: url ?? null };
}

function buildFiles(urls: string[]): { files: Array<{ type: 'external'; name: string; external: { url: string } }> } {
	return {
		files: urls.map((url, i) => ({
			type: 'external' as const,
			name: `Image ${i + 1}`,
			external: { url },
		})),
	};
}

// =============================================================================
// PROPERTY MAPPING
// =============================================================================

/**
 * Build Notion properties object from a listing
 * Maps to "New Home 2026" database schema
 */
export function buildNotionProperties(listing: Listing): Record<string, unknown> {
	return {
		// Title (required)
		Name: buildTitle(listing.address),

		// Core details
		Price: buildNumber(listing.price),
		Bedrooms: buildNumber(listing.bedrooms),
		Bathrooms: buildNumber(listing.bathrooms),
		'Floor Area': buildNumber(listing.floorAreaSqm),
		Score: buildNumber(listing.assessedScore ?? listing.scores?._overall ?? null),

		// Selects
		EPC: buildSelect(listing.epcRating),
		Garden: buildSelect(listing.scores?.factors?.gardenType ?? null),
		Heating: buildSelect(listing.scores?.factors?.heatingType ?? null),
		Pets: buildSelect(listing.scores?.factors?.petPolicy ?? null),

		// Text fields
		Type: buildRichText(listing.propertyType),
		Region: buildRichText(listing.region),
		Notes: buildRichText(listing.notes?.join('\n')),

		// Address (place type not API-supported, using text field with coordinates for Notion AI)
		'Address Text': buildRichText(`${listing.address}, ${listing.postcode} [${listing.location.lat},${listing.location.lng}]`),

		// Links
		URL: buildUrl(listing.url),
		'Google Maps': buildUrl(listing.googleMapsUrl),
		'Google Street View': buildUrl(listing.googleMapsStreetViewUrl),

		// AI Assessment summary
		'Notes (AI)': buildRichText(listing.assessment?.reasoning ?? null),

		// Images (all of them - prepend satellite if available, then property photos)
		Images: buildFiles(
			listing.mapViews?.satellite?.remote && process.env['MAPBOX_ACCESS_TOKEN']
				? [`${listing.mapViews.satellite.remote}?access_token=${process.env['MAPBOX_ACCESS_TOKEN']}`, ...listing.images.map((i) => i.remote)]
				: listing.images.map((i) => i.remote),
		),
	};
}

// =============================================================================
// API FUNCTIONS
// =============================================================================

/**
 * Create a new page in the Notion database
 * @returns The created page ID
 */
export async function createNotionPage(config: NotionConfig, listing: Listing): Promise<string> {
	log.notion.info('Creating page', { id: listing.id, address: listing.address.slice(0, 50) });

	const properties = buildNotionProperties(listing);

	const page = await notionFetch<NotionPage>(config, '/pages', {
		method: 'POST',
		body: JSON.stringify({
			parent: { database_id: config.databaseId },
			properties,
		}),
	});

	log.notion.success('Created page', { listingId: listing.id, pageId: page.id });
	return page.id;
}

/**
 * Update an existing page in the Notion database
 */
export async function updateNotionPage(config: NotionConfig, pageId: string, listing: Listing): Promise<void> {
	log.notion.info('Updating page', { pageId, address: listing.address.slice(0, 50) });

	const properties = buildNotionProperties(listing);

	await notionFetch<NotionPage>(config, `/pages/${pageId}`, {
		method: 'PATCH',
		body: JSON.stringify({ properties }),
	});

	log.notion.success('Updated page', { listingId: listing.id, pageId });
}

/**
 * Query all pages in the database to find existing listings by their ID
 * @returns Map of listing ID -> Notion page ID
 */
export async function queryExistingPages(config: NotionConfig): Promise<Map<string, string>> {
	log.notion.info('Querying existing pages');

	const existingPages = new Map<string, string>();
	let cursor: string | null = null;

	do {
		const body: Record<string, unknown> = { page_size: 100 };
		if (cursor) body['start_cursor'] = cursor;

		const response = await notionFetch<NotionQueryResponse>(config, `/databases/${config.databaseId}/query`, {
			method: 'POST',
			body: JSON.stringify(body),
		});

		for (const page of response.results) {
			// Extract listing ID from URL property if present
			const urlProp = page.properties['URL'] as { url?: string } | undefined;
			if (urlProp?.url) {
				// URL format: https://www.rightmove.co.uk/properties/{id}
				const match = urlProp.url.match(/\/properties\/(\d+)/);
				if (match?.[1]) {
					existingPages.set(match[1], page.id);
				}
			}
		}

		cursor = response.has_more ? response.next_cursor : null;
	} while (cursor);

	log.notion.info('Found existing pages', { count: existingPages.size });
	return existingPages;
}

/**
 * Validate that the database exists and is accessible
 */
export async function validateDatabase(config: NotionConfig): Promise<boolean> {
	try {
		await notionFetch<{ id: string }>(config, `/databases/${config.databaseId}`);
		log.notion.success('Database validated', { databaseId: config.databaseId });
		return true;
	} catch {
		log.notion.error('Database validation failed', { databaseId: config.databaseId });
		return false;
	}
}
