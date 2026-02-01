/**
 * EPC address normalization and matching tests
 */

import { describe, expect, test } from 'bun:test';
import { addressesMatch, levenshteinDistance, type NormalizedAddress, normalizeAddress } from '../src/pipeline/enrich/epc.js';

describe('normalizeAddress', () => {
	test('extracts basic house number and street', () => {
		const result = normalizeAddress('123 High Street');
		expect(result.number).toBe('123');
		expect(result.numberSuffix).toBe('');
		expect(result.flat).toBe('');
		expect(result.streetName).toBe('high');
		expect(result.streetType).toBe('street');
	});

	test('extracts flat number before house number', () => {
		const result = normalizeAddress('Flat 2, 123 High Street');
		expect(result.number).toBe('123');
		expect(result.numberSuffix).toBe('');
		expect(result.flat).toBe('2');
		expect(result.streetName).toBe('high');
		expect(result.streetType).toBe('street');
	});

	test('extracts unit number', () => {
		const result = normalizeAddress('Unit 5, 45 Main Road');
		expect(result.number).toBe('45');
		expect(result.flat).toBe('5');
		expect(result.streetName).toBe('main');
		expect(result.streetType).toBe('road');
	});

	test('handles apartment abbreviation', () => {
		const result = normalizeAddress('Apt 3, 78 Oak Lane');
		expect(result.number).toBe('78');
		expect(result.flat).toBe('3');
		expect(result.streetName).toBe('oak');
		expect(result.streetType).toBe('lane');
	});

	test('extracts letter suffix attached to number', () => {
		const result = normalizeAddress('123A High Road');
		expect(result.number).toBe('123');
		expect(result.numberSuffix).toBe('a');
		expect(result.streetName).toBe('high');
		expect(result.streetType).toBe('road');
	});

	test('extracts letter suffix with slash separator', () => {
		const result = normalizeAddress('12/B High Lane');
		expect(result.number).toBe('12');
		expect(result.numberSuffix).toBe('b');
		expect(result.streetName).toBe('high');
		expect(result.streetType).toBe('lane');
	});

	test('extracts letter suffix with dash separator', () => {
		const result = normalizeAddress('12-A High Lane');
		expect(result.number).toBe('12');
		expect(result.numberSuffix).toBe('a');
		expect(result.streetName).toBe('high');
		expect(result.streetType).toBe('lane');
	});

	test('normalizes suffix to lowercase', () => {
		const result = normalizeAddress('123B High Road');
		expect(result.numberSuffix).toBe('b');
	});

	test('expands rd abbreviation to road', () => {
		const result = normalizeAddress('45 Main Rd');
		expect(result.streetType).toBe('road');
	});

	test('expands st abbreviation to street', () => {
		const result = normalizeAddress('45 Main St');
		expect(result.streetType).toBe('street');
	});

	test('expands ave abbreviation to avenue', () => {
		const result = normalizeAddress('45 Main Ave');
		expect(result.streetType).toBe('avenue');
	});

	test('expands cres abbreviation to crescent', () => {
		const result = normalizeAddress('45 Main Cres');
		expect(result.streetType).toBe('crescent');
	});

	test('keeps full street type unchanged', () => {
		const result = normalizeAddress('45 Main Road');
		expect(result.streetType).toBe('road');
	});

	test('handles multi-word street names', () => {
		const result = normalizeAddress('123 Old Mill Road');
		expect(result.streetName).toBe('old');
		expect(result.streetType).toBe('road');
	});

	test('handles address with no street type', () => {
		const result = normalizeAddress('123 The Green');
		expect(result.number).toBe('123');
		expect(result.streetName).toBe('the');
		expect(result.streetType).toBe('');
	});

	test('preserves original address', () => {
		const original = 'Flat 2, 123A High Street';
		const result = normalizeAddress(original);
		expect(result.original).toBe(original);
	});

	test('handles flat number with letter suffix', () => {
		const result = normalizeAddress('Flat 2A, 123 High Street');
		expect(result.flat).toBe('2a');
		expect(result.number).toBe('123');
	});
});

describe('levenshteinDistance', () => {
	test('returns 0 for identical strings', () => {
		expect(levenshteinDistance('high', 'high')).toBe(0);
	});

	test('returns string length for empty comparison', () => {
		expect(levenshteinDistance('high', '')).toBe(4);
		expect(levenshteinDistance('', 'high')).toBe(4);
	});

	test('returns 1 for single character difference', () => {
		expect(levenshteinDistance('high', 'hgh')).toBe(1); // deletion
		expect(levenshteinDistance('high', 'highs')).toBe(1); // insertion
		expect(levenshteinDistance('high', 'hish')).toBe(1); // substitution
	});

	test('returns 2 for two character differences', () => {
		expect(levenshteinDistance('high', 'hg')).toBe(2);
		expect(levenshteinDistance('main', 'mein')).toBe(1);
	});

	test('handles completely different strings', () => {
		expect(levenshteinDistance('abc', 'xyz')).toBe(3);
	});
});

describe('addressesMatch', () => {
	function normalize(addr: string): NormalizedAddress {
		return normalizeAddress(addr);
	}

	test('matches identical addresses', () => {
		expect(addressesMatch(normalize('123 High Road'), normalize('123 High Road'))).toBe(true);
	});

	test('matches with different abbreviations', () => {
		expect(addressesMatch(normalize('123 High Road'), normalize('123 High Rd'))).toBe(true);
	});

	test('rejects different street types', () => {
		expect(addressesMatch(normalize('123 High Road'), normalize('123 High Street'))).toBe(false);
	});

	test('rejects different house numbers', () => {
		expect(addressesMatch(normalize('123 High Road'), normalize('124 High Road'))).toBe(false);
	});

	test('matches with minor typo in street name (Levenshtein = 1)', () => {
		expect(addressesMatch(normalize('123 Hgh Road'), normalize('123 High Road'))).toBe(true);
	});

	test('rejects with major typo in street name (Levenshtein > 1)', () => {
		expect(addressesMatch(normalize('123 Hg Road'), normalize('123 High Road'))).toBe(false);
	});

	test('rejects different number suffixes', () => {
		expect(addressesMatch(normalize('123A High Road'), normalize('123B High Road'))).toBe(false);
	});

	test('matches same number suffixes', () => {
		expect(addressesMatch(normalize('123A High Road'), normalize('123a High Rd'))).toBe(true);
	});

	test('matches flat addresses', () => {
		expect(addressesMatch(normalize('Flat 2, 123 High Road'), normalize('Flat 2, 123 High Rd'))).toBe(true);
	});

	test('rejects different flat numbers', () => {
		expect(addressesMatch(normalize('Flat 2, 123 High Road'), normalize('Flat 3, 123 High Road'))).toBe(false);
	});

	test('rejects when one has flat and other does not', () => {
		expect(addressesMatch(normalize('Flat 2, 123 High Road'), normalize('123 High Road'))).toBe(false);
	});

	test('rejects addresses without house numbers', () => {
		expect(addressesMatch(normalize('High Road'), normalize('High Road'))).toBe(false);
	});

	test('handles mixed case addresses', () => {
		expect(addressesMatch(normalize('123 HIGH ROAD'), normalize('123 high road'))).toBe(true);
	});

	test('handles addresses with punctuation', () => {
		expect(addressesMatch(normalize('123, High Road.'), normalize('123 High Road'))).toBe(true);
	});
});
