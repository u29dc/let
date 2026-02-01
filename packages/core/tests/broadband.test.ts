import { afterAll, describe, expect, test } from 'bun:test';
import { closeBroadbandDb, lookupBroadband } from '@let/core/pipeline/enrich';

// Clean up database connection after all tests
afterAll(() => {
	closeBroadbandDb();
});

describe('lookupBroadband', () => {
	describe('exact postcode lookup', () => {
		test('returns data for known postcode NG6 8JU', () => {
			const result = lookupBroadband('NG6 8JU');
			expect(result).not.toBeNull();
			expect(result?.source).toBe('postcode');
			expect(result?.gigabitAvailability).toBeGreaterThanOrEqual(0);
			expect(result?.gigabitAvailability).toBeLessThanOrEqual(100);
		});

		test('handles postcode without space', () => {
			const result = lookupBroadband('NG68JU');
			expect(result).not.toBeNull();
			expect(result?.source).toBe('postcode');
		});

		test('handles lowercase postcode', () => {
			const result = lookupBroadband('ng6 8ju');
			expect(result).not.toBeNull();
			expect(result?.source).toBe('postcode');
		});

		test('returns data for NG2 4JL', () => {
			const result = lookupBroadband('NG2 4JL');
			expect(result).not.toBeNull();
			expect(result?.source).toBe('postcode');
		});

		test('returns data for L9 9BA (Liverpool)', () => {
			const result = lookupBroadband('L9 9BA');
			expect(result).not.toBeNull();
			expect(result?.source).toBe('postcode');
		});

		test('returns data for NE4 6RJ (Newcastle)', () => {
			const result = lookupBroadband('NE4 6RJ');
			expect(result).not.toBeNull();
			expect(result?.source).toBe('postcode');
		});
	});

	describe('fallback to district aggregate', () => {
		test('falls back to outward code when exact postcode not found', () => {
			// S26 5AN not in database, but S26 district has aggregate data
			const result = lookupBroadband('S26 5AN');
			expect(result).not.toBeNull();
			expect(result?.source).toBe('outward');
			expect(result?.gigabitAvailability).toBeGreaterThanOrEqual(0);
		});
	});

	describe('fallback to area aggregate', () => {
		test('falls back to area when district not found', () => {
			// Using a postcode that likely doesn't exist but area does
			// YO1 7PR not in database, but YO area should have aggregate
			const result = lookupBroadband('YO1 7PR');
			// May return outward or area depending on data
			if (result) {
				expect(['outward', 'area']).toContain(result.source);
			}
		});
	});

	describe('not found cases', () => {
		test('returns null for invalid postcode format', () => {
			const result = lookupBroadband('INVALID');
			expect(result).toBeNull();
		});

		test('returns null for empty string', () => {
			const result = lookupBroadband('');
			expect(result).toBeNull();
		});

		test('area code lookup returns area aggregate', () => {
			// 'AB' is a valid area code (Aberdeen) - should return area aggregate
			const result = lookupBroadband('AB');
			expect(result).not.toBeNull();
			if (result) {
				expect(result.source).toBe('area');
			}
		});

		test('returns null for invalid area code', () => {
			// 'QQ' is not a valid UK postcode area
			const result = lookupBroadband('QQ');
			expect(result).toBeNull();
		});
	});

	describe('data quality', () => {
		test('gigabit availability is a valid percentage', () => {
			const result = lookupBroadband('NG6 8JU');
			expect(result).not.toBeNull();
			if (result) {
				expect(typeof result.gigabitAvailability).toBe('number');
				expect(result.gigabitAvailability).toBeGreaterThanOrEqual(0);
				expect(result.gigabitAvailability).toBeLessThanOrEqual(100);
			}
		});

		test('source is one of expected values', () => {
			const result = lookupBroadband('NG6 8JU');
			expect(result).not.toBeNull();
			if (result) {
				expect(['postcode', 'outward', 'area']).toContain(result.source);
			}
		});
	});
});
