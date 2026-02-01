import { afterEach, describe, expect, test } from 'bun:test';
import { buildSearchApiUrl, lookupLocation, resetApiRateLimiter, tokenizeLocation } from '@let/core/pipeline/fetch';

function setMockFetch(handler: (input: RequestInfo | URL, init?: RequestInit) => Promise<Response>): void {
	const mock = ((input: RequestInfo | URL, init?: RequestInit) => handler(input, init)) as typeof fetch;
	mock.preconnect = () => {};
	globalThis.fetch = mock;
}

describe('tokenizeLocation', () => {
	test('tokenizes short region name', () => {
		expect(tokenizeLocation('York')).toBe('YO/RK');
	});

	test('tokenizes longer region name', () => {
		expect(tokenizeLocation('Newcastle')).toBe('NE/WC/AS/TL/E');
	});

	test('handles odd-length names', () => {
		expect(tokenizeLocation('Durham')).toBe('DU/RH/AM');
	});

	test('handles single character', () => {
		expect(tokenizeLocation('A')).toBe('A');
	});

	test('handles two characters', () => {
		expect(tokenizeLocation('AB')).toBe('AB');
	});

	test('handles three characters', () => {
		expect(tokenizeLocation('ABC')).toBe('AB/C');
	});

	test('ignores case', () => {
		expect(tokenizeLocation('york')).toBe('YO/RK');
		expect(tokenizeLocation('YORK')).toBe('YO/RK');
		expect(tokenizeLocation('YoRk')).toBe('YO/RK');
	});

	test('strips non-alphabetic characters', () => {
		expect(tokenizeLocation('St. Albans')).toBe('ST/AL/BA/NS');
		expect(tokenizeLocation('Newcastle-upon-Tyne')).toBe('NE/WC/AS/TL/EU/PO/NT/YN/E');
	});

	test('handles spaces', () => {
		expect(tokenizeLocation('Milton Keynes')).toBe('MI/LT/ON/KE/YN/ES');
	});
});

describe('buildSearchApiUrl', () => {
	test('builds basic URL with required params', () => {
		const url = buildSearchApiUrl({ locationIdentifier: 'REGION^1498' });

		expect(url).toContain('https://www.rightmove.co.uk/api/_search');
		expect(url).toContain('locationIdentifier=REGION%5E1498');
		expect(url).toContain('channel=RENT');
		expect(url).toContain('numberOfPropertiesPerPage=24');
	});

	test('includes optional params when provided', () => {
		const url = buildSearchApiUrl({
			locationIdentifier: 'REGION^1498',
			minBedrooms: 2,
			maxPrice: 1400,
			radius: 3,
		});

		expect(url).toContain('minBedrooms=2');
		expect(url).toContain('maxPrice=1400');
		expect(url).toContain('radius=3');
	});

	test('includes property types', () => {
		const url = buildSearchApiUrl({
			locationIdentifier: 'REGION^1498',
			propertyTypes: ['detached', 'semi-detached'],
		});

		expect(url).toContain('propertyTypes=detached');
		expect(url).toContain('propertyTypes=semi-detached');
	});

	test('includes includeLetAgreed flag', () => {
		const url = buildSearchApiUrl({
			locationIdentifier: 'REGION^1498',
			includeLetAgreed: false,
		});

		expect(url).toContain('includeLetAgreed=false');
	});

	test('supports pagination index', () => {
		const url = buildSearchApiUrl({
			locationIdentifier: 'REGION^1498',
			index: 24,
		});

		expect(url).toContain('index=24');
	});
});

describe('lookupLocation', () => {
	const originalFetch = globalThis.fetch;

	afterEach(() => {
		globalThis.fetch = originalFetch;
		resetApiRateLimiter();
	});

	test.serial('retries on 429 and succeeds', async () => {
		resetApiRateLimiter();

		let calls = 0;
		setMockFetch(async () => {
			calls += 1;
			if (calls === 1) {
				return new Response('rate limit', { status: 429, headers: { 'Retry-After': '0' } });
			}
			return new Response(
				JSON.stringify({
					typeAheadLocations: [
						{
							displayName: 'York',
							locationIdentifier: 'REGION^1498',
							normalisedSearchTerm: 'york',
						},
					],
				}),
				{ status: 200, headers: { 'Content-Type': 'application/json' } },
			);
		});

		const result = await lookupLocation('York');
		expect(calls).toBe(2);
		expect(result.success).toBe(true);
		if (result.success) {
			expect(result.locations[0]?.locationIdentifier).toBe('REGION^1498');
		}
	});
});
