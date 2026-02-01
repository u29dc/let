import { afterEach, beforeEach, describe, expect, test } from 'bun:test';
import { DEFAULT_DELAY_MS, fetchWithRateLimit, resetRateLimiter, setFetchDelay } from '@let/core/pipeline/fetch';

function setMockFetch(handler: (input: RequestInfo | URL, init?: RequestInit) => Promise<Response>): void {
	const mock = ((input: RequestInfo | URL, init?: RequestInit) => handler(input, init)) as typeof fetch;
	mock.preconnect = () => {};
	globalThis.fetch = mock;
}

const originalFetch = globalThis.fetch;

describe('fetchWithRateLimit', () => {
	beforeEach(() => {
		resetRateLimiter();
		setFetchDelay(0);
	});

	afterEach(() => {
		globalThis.fetch = originalFetch;
		setFetchDelay(DEFAULT_DELAY_MS);
		resetRateLimiter();
	});

	test.serial('retries on 429 and succeeds', async () => {
		let calls = 0;
		setMockFetch(async () => {
			calls += 1;
			if (calls === 1) {
				return new Response('rate limit', { status: 429, headers: { 'Retry-After': '0' } });
			}
			return new Response('<html>ok</html>', { status: 200 });
		});

		const result = await fetchWithRateLimit('https://example.com');
		expect(calls).toBe(2);
		expect(result.success).toBe(true);
		if (result.success) {
			expect(result.html).toBe('<html>ok</html>');
		}
	});
});
