/**
 * HTML fetching utilities for Rightmove scraping
 *
 * Rightmove returns 429 errors after 3-4 rapid requests.
 * A 3-second delay between requests is safe.
 * Uses centralized rate limiter with jitter for anti-fingerprinting.
 */

import { log } from '@let/core/utils/logger';

// =============================================================================
// RATE LIMITER
// =============================================================================

/** Sleep for a specified duration */
const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

/** Default jitter as fraction of base delay (15%) */
const DEFAULT_JITTER = 0.15;

/**
 * Calculate delay with random jitter
 *
 * @param baseMs - Base delay in milliseconds
 * @param jitterFraction - Jitter as fraction of base (0.15 = 15%)
 * @returns Jittered delay in milliseconds
 */
export function getJitteredDelay(baseMs: number, jitterFraction: number = DEFAULT_JITTER): number {
	const jitter = baseMs * jitterFraction;
	return Math.round(baseMs + (Math.random() * 2 - 1) * jitter);
}

/**
 * Create a resettable rate limiter with jitter
 */
export function createResettableRateLimiter(delayMs: number, jitter: number = DEFAULT_JITTER) {
	let chain: Promise<void> = Promise.resolve();
	let lastTime = 0;

	return {
		async throttle(): Promise<void> {
			chain = chain.then(async () => {
				const elapsed = Date.now() - lastTime;
				const targetDelay = getJitteredDelay(delayMs, jitter);
				if (elapsed < targetDelay && lastTime > 0) {
					await sleep(targetDelay - elapsed);
				}
				lastTime = Date.now();
			});
			await chain;
		},
		reset(): void {
			lastTime = 0;
			chain = Promise.resolve();
		},
	};
}

// =============================================================================
// HTTP FETCHING
// =============================================================================

/** Default delay between requests (milliseconds) */
export const DEFAULT_DELAY_MS = 3000;

/** Concurrency-safe rate limiter with jitter for HTML fetching */
let htmlRateLimiter = createResettableRateLimiter(DEFAULT_DELAY_MS);

/**
 * Set the fetch delay (call at startup if using --delay CLI flag)
 */
export function setFetchDelay(delayMs: number): void {
	htmlRateLimiter = createResettableRateLimiter(delayMs);
}

/** Maximum retries for transient errors */
let maxRetries = 3;

/**
 * Set maximum retries for fetch operations
 */
export function setFetchMaxRetries(value: number): void {
	const normalized = Math.max(1, Math.floor(value));
	maxRetries = normalized;
}

/** Delay between retries (milliseconds) */
const RETRY_DELAY_MS = 5000;

/** User agent to mimic a real browser */
const USER_AGENT = 'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36';

/** Fetch timeout in milliseconds */
const FETCH_TIMEOUT_MS = 30000;

/** Result of a fetch operation */
export type FetchResult = { success: true; html: string; status: number } | { success: false; error: string; status?: number };

/** Handle a successful response */
async function handleSuccessResponse(response: Response): Promise<FetchResult> {
	const html = await response.text();
	return { success: true, html, status: response.status };
}

/** Calculate exponential backoff with jitter */
function calculateBackoff(attempt: number, response?: Response): number {
	const retryAfter = response?.headers.get('Retry-After');
	if (retryAfter) {
		const seconds = Number.parseInt(retryAfter, 10);
		if (!Number.isNaN(seconds)) {
			return seconds * 1000;
		}
	}
	return RETRY_DELAY_MS * 2 ** (attempt - 1) + Math.random() * 500;
}

/** Handle rate limit (429) response, returns true if should retry */
async function handleRateLimitResponse(attempt: number, response?: Response): Promise<boolean> {
	log.fetch.warn('Rate limited by server', { attempt, maxRetries });
	if (attempt < maxRetries) {
		const backoff = calculateBackoff(attempt, response);
		log.fetch.debug('Backing off', { backoff: Math.round(backoff) });
		await sleep(backoff);
		return true;
	}
	return false;
}

/** Handle server error response, returns true if should retry */
async function handleServerError(status: number, attempt: number): Promise<boolean> {
	log.fetch.warn('Server error', { status, attempt, maxRetries });
	if (attempt < maxRetries) {
		const backoff = calculateBackoff(attempt);
		await sleep(backoff);
		return true;
	}
	return false;
}

/** Single fetch attempt result */
type AttemptResult = { done: true; result: FetchResult } | { done: false; error: string; status?: number };

/** Execute a single fetch attempt */
async function executeAttempt(url: string, attempt: number): Promise<AttemptResult> {
	const controller = new AbortController();
	const timeout = setTimeout(() => controller.abort(), FETCH_TIMEOUT_MS);

	let response: Response;
	try {
		response = await fetch(url, {
			signal: controller.signal,
			headers: {
				'User-Agent': USER_AGENT,
				Accept: 'text/html,application/xhtml+xml,application/xml;q=0.9,image/webp,*/*;q=0.8',
				'Accept-Language': 'en-GB,en;q=0.9',
				'Cache-Control': 'no-cache',
			},
		});
	} finally {
		clearTimeout(timeout);
	}

	if (response.ok) return { done: true, result: await handleSuccessResponse(response) };

	if (response.status === 429) {
		if (await handleRateLimitResponse(attempt, response)) return { done: false, error: 'Rate limited (429)', status: response.status };
	} else if (response.status >= 400 && response.status < 500) {
		return { done: true, result: { success: false, error: `HTTP ${response.status}: ${response.statusText}`, status: response.status } };
	} else if (await handleServerError(response.status, attempt)) {
		return { done: false, error: `HTTP ${response.status}: ${response.statusText}`, status: response.status };
	}

	return { done: false, error: `HTTP ${response.status}: ${response.statusText}`, status: response.status };
}

/**
 * Fetch a URL with rate limiting and retries
 *
 * @param url - URL to fetch
 * @returns HTML content or error
 */
export async function fetchWithRateLimit(url: string): Promise<FetchResult> {
	await htmlRateLimiter.throttle();

	let lastError = 'Unknown error';
	let lastStatus: number | undefined;

	for (let attempt = 1; attempt <= maxRetries; attempt++) {
		try {
			const result = await executeAttempt(url, attempt);
			if (result.done) return result.result;
			lastError = result.error;
			lastStatus = result.status;
		} catch (e) {
			lastError = e instanceof Error ? e.message : 'Network error';
			log.fetch.error('Network error', { error: lastError, attempt, maxRetries });
			if (attempt < maxRetries) await sleep(RETRY_DELAY_MS);
		}
	}

	return lastStatus !== undefined ? { success: false, error: lastError, status: lastStatus } : { success: false, error: lastError };
}

/**
 * Build Rightmove listing URL from ID
 */
export function buildListingUrl(id: string): string {
	return `https://www.rightmove.co.uk/properties/${id}`;
}

/**
 * Build Rightmove search URL from parameters
 */
export function buildSearchUrl(params: {
	locationIdentifier: string;
	minBedrooms?: number;
	maxBedrooms?: number;
	minPrice?: number;
	maxPrice?: number;
	propertyTypes?: string[];
	includeLetAgreed?: boolean;
	radius?: number;
	dontShow?: string[];
	mustHave?: string[];
	index?: number;
}): string {
	const base = 'https://www.rightmove.co.uk/property-to-rent/find.html';
	const searchParams = new URLSearchParams();

	searchParams.set('locationIdentifier', params.locationIdentifier);
	searchParams.set('sortType', '6');

	if (params.minBedrooms !== undefined) {
		searchParams.set('minBedrooms', params.minBedrooms.toString());
	}
	if (params.maxBedrooms !== undefined) {
		searchParams.set('maxBedrooms', params.maxBedrooms.toString());
	}
	if (params.minPrice !== undefined) {
		searchParams.set('minPrice', params.minPrice.toString());
	}
	if (params.maxPrice !== undefined) {
		searchParams.set('maxPrice', params.maxPrice.toString());
	}
	if (params.propertyTypes?.length) {
		searchParams.set('propertyTypes', params.propertyTypes.join(','));
	}
	if (params.includeLetAgreed !== undefined) {
		searchParams.set('includeLetAgreed', params.includeLetAgreed.toString());
	}
	if (params.radius !== undefined) {
		searchParams.set('radius', params.radius.toString());
	}
	if (params.dontShow?.length) {
		searchParams.set('dontShow', params.dontShow.join(','));
	}
	if (params.mustHave?.length) {
		searchParams.set('mustHave', params.mustHave.join(','));
	}
	if (params.index !== undefined && params.index > 0) {
		searchParams.set('index', params.index.toString());
	}

	return `${base}?${searchParams.toString()}`;
}

/**
 * Reset rate limiter (useful for testing)
 */
export function resetRateLimiter(): void {
	htmlRateLimiter.reset();
}

/**
 * Sleep utility (exported for use in other modules)
 */
export { sleep };
