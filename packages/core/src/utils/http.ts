/**
 * Generic HTTP utilities
 *
 * Concurrency-safe rate limiting with promise chaining.
 * Used by pipeline modules for rate-limited API access.
 */

// =============================================================================
// UTILITIES
// =============================================================================

/** Sleep for a specified duration */
export const sleep = (ms: number): Promise<void> => new Promise((resolve) => setTimeout(resolve, ms));

/** Default jitter as fraction of base delay (±15%) */
const DEFAULT_JITTER = 0.15;

/**
 * Calculate delay with random jitter
 *
 * @param baseMs - Base delay in milliseconds
 * @param jitterFraction - Jitter as fraction of base (0.15 = ±15%)
 * @returns Jittered delay in milliseconds
 */
export function getJitteredDelay(baseMs: number, jitterFraction: number = DEFAULT_JITTER): number {
	const jitter = baseMs * jitterFraction;
	return Math.round(baseMs + (Math.random() * 2 - 1) * jitter);
}

// =============================================================================
// RATE LIMITER
// =============================================================================

/**
 * Create a rate limiter instance with jitter
 *
 * Uses promise chaining for concurrency safety - multiple concurrent calls
 * will be serialized and each will wait the appropriate time.
 *
 * @param delayMs - Base delay between requests
 * @param jitter - Jitter fraction (default 0.15 = ±15%)
 * @returns A throttle function that enforces the rate limit
 */
export function createRateLimiter(delayMs: number, jitter: number = DEFAULT_JITTER): () => Promise<void> {
	let chain: Promise<void> = Promise.resolve();
	let lastTime = 0;

	return async function throttle(): Promise<void> {
		chain = chain.then(async () => {
			const elapsed = Date.now() - lastTime;
			const targetDelay = getJitteredDelay(delayMs, jitter);
			if (elapsed < targetDelay && lastTime > 0) {
				await sleep(targetDelay - elapsed);
			}
			lastTime = Date.now();
		});
		await chain;
	};
}

/**
 * Create a resettable rate limiter with jitter (useful for testing)
 *
 * @param delayMs - Base delay between requests
 * @param jitter - Jitter fraction (default 0.15 = ±15%)
 * @returns Object with throttle() and reset() methods
 */
export function createResettableRateLimiter(
	delayMs: number,
	jitter: number = DEFAULT_JITTER,
): {
	throttle: () => Promise<void>;
	reset: () => void;
} {
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
// BACKOFF UTILITIES
// =============================================================================

/**
 * Calculate exponential backoff with jitter
 *
 * @param attempt - Current attempt number (1-based)
 * @param baseMs - Base delay in milliseconds
 * @param retryAfterHeader - Optional Retry-After header value
 * @returns Backoff delay in milliseconds
 */
export function calculateBackoff(attempt: number, baseMs: number = 5000, retryAfterHeader?: string | null): number {
	// Check for Retry-After header first
	if (retryAfterHeader) {
		const seconds = Number.parseInt(retryAfterHeader, 10);
		if (!Number.isNaN(seconds)) {
			return seconds * 1000;
		}
	}
	// Exponential backoff with jitter: base * 2^(attempt-1) + random(0-500ms)
	return baseMs * 2 ** (attempt - 1) + Math.random() * 500;
}
