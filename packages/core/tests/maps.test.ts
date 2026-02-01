import { afterEach, beforeEach, describe, expect, test } from 'bun:test';
import { mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fetchMapViews } from '@let/core/pipeline/fetch';

function setMockFetch(handler: (input: RequestInfo | URL, init?: RequestInit) => Promise<Response>): void {
	const mock = ((input: RequestInfo | URL, init?: RequestInit) => handler(input, init)) as typeof fetch;
	mock.preconnect = () => {};
	globalThis.fetch = mock;
}

const originalFetch = globalThis.fetch;
const originalToken = process.env['MAPBOX_ACCESS_TOKEN'];

describe('fetchMapViews', () => {
	beforeEach(() => {
		process.env['MAPBOX_ACCESS_TOKEN'] = 'test-token';
	});

	afterEach(() => {
		globalThis.fetch = originalFetch;
		if (originalToken === undefined) {
			delete process.env['MAPBOX_ACCESS_TOKEN'];
		} else {
			process.env['MAPBOX_ACCESS_TOKEN'] = originalToken;
		}
	});

	test.serial('retries on 429 for both views', async () => {
		let calls = 0;
		setMockFetch(async () => {
			calls += 1;
			return new Response('rate limit', { status: 429, headers: { 'Retry-After': '0' } });
		});

		const cacheDir = mkdtempSync(join(tmpdir(), 'let-maps-'));
		const result = await fetchMapViews('123', 51.5, -0.1, cacheDir);

		expect(result.success).toBe(true);
		if (result.success) {
			expect(result.mapViews.satellite.local).toBe(null);
			expect(result.mapViews.street.local).toBe(null);
		}
		expect(calls).toBe(4);
	});
});
