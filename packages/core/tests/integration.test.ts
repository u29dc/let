/**
 * Integration tests using real HTML fixtures
 *
 * These tests validate the full scraping pipeline against
 * realistic Rightmove HTML structure.
 */

import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { scrapeListing } from '@let/core/pipeline/parse';

const FIXTURES_DIR = join(import.meta.dirname, 'fixtures', 'listings');

function loadFixture(id: string): string {
	return readFileSync(join(FIXTURES_DIR, `${id}.html`), 'utf-8');
}

describe('Integration: Full scraping pipeline', () => {
	describe('Testford listing (123456789)', () => {
		test('extracts all fields from complete listing', async () => {
			const html = loadFixture('123456789');
			const result = await scrapeListing('123456789', html);

			expect(result.success).toBe(true);
			if (!result.success) return;

			const listing = result.listing;

			// Identity
			expect(listing.id).toMatch(/^[0-9a-f-]{36}$/i);
			expect(listing.portalIds.rightmove).toBe('123456789');
			expect(listing.url).toBe('https://www.rightmove.co.uk/properties/123456789');

			// Location
			expect(listing.location.lat).toBe(53.9614);
			expect(listing.location.lng).toBe(-1.0739);
			expect(listing.location.pinType).toBe('ACCURATE_POINT');
			expect(listing.postcode).toBe('TF1 2AB');
			expect(listing.address).toBe('Maple Avenue, Testford');

			// Property details
			expect(listing.price).toBe(1250);
			expect(listing.priceDisplay).toBe('£1,250 pcm');
			expect(listing.bedrooms).toBe(3);
			expect(listing.bathrooms).toBe(2);
			expect(listing.propertyType).toBe('Terraced');

			// Content (description is lowercase combined text)
			expect(listing.description).toContain('3 bedroom terraced house');
			expect(listing.description).toContain('private rear garden');
			expect(listing.description).toContain('epc rating');
			expect(listing.description).toContain('pets considered');
			expect(listing.notes).toEqual([]); // Empty until AI enrichment

			// Media
			expect(listing.images).toHaveLength(4);
			expect(listing.images[0]?.remote).toContain('IMG_00'); // Remote URLs with null local
			expect(listing.floorplan.remote).toContain('FLP_00');
			expect(listing.epc.remote).toContain('EPC_00');

			// Transport & utilities
			expect(listing.nearestStations).toHaveLength(2);
			expect(listing.nearestStations[0]?.name).toBe('Testford');
			expect(listing.nearestStations[0]?.distance).toBe(0.8);
			expect(listing.gigabitAvailability).toBeNull(); // Populated later by Ofcom lookup

			// Dates
			expect(listing.listedDate).toBe('2024-01-15');

			// Lettings
			expect(listing.lettings.availableDate).toBe('01/02/2024');
			expect(listing.lettings.deposit).toBe(1442);

			// Agent
			expect(listing.agent.name).toBe('Testford Property Lettings');
			expect(listing.agent.phone).toBe('01234 567890');

			// Metadata
			expect(listing.extractionStatus).toBe('success');
		});
	});

	describe('Millbrook listing (987654321)', () => {
		test('handles missing optional fields gracefully', async () => {
			const html = loadFixture('987654321');
			const result = await scrapeListing('987654321', html);

			expect(result.success).toBe(true);
			if (!result.success) return;

			const listing = result.listing;

			// Basic fields
			expect(listing.id).toMatch(/^[0-9a-f-]{36}$/i);
			expect(listing.portalIds.rightmove).toBe('987654321');
			expect(listing.postcode).toBe('MB1 3CD');
			expect(listing.price).toBe(950);
			expect(listing.propertyType).toBe('Semi-Detached');

			// Approximate location
			expect(listing.location.pinType).toBe('APPROXIMATE_POINT');

			// HTML entities decoded, lowercase
			expect(listing.description).toContain('south-facing rear garden');
			expect(listing.description).not.toContain('&amp;');

			// Missing optional fields are null
			expect(listing.floorplan.remote).toBeNull();
			expect(listing.epc.remote).toBeNull();
			expect(listing.lettings.deposit).toBeNull();
			expect(listing.agent.phone).toBeNull();

			// Images (remote URL objects with null local)
			expect(listing.images).toHaveLength(2);
			expect(typeof listing.images[0]?.remote).toBe('string');

			// Reduced date parsing
			expect(listing.listedDate).toBe('2023-12-20');

			// Still success despite missing optional fields
			expect(listing.extractionStatus).toBe('success');
		});

		test('extracts EPC from description text', async () => {
			const html = loadFixture('987654321');
			const result = await scrapeListing('987654321', html);

			expect(result.success).toBe(true);
			if (!result.success) return;

			// EPC info is in description (lowercase)
			expect(result.listing.description).toContain('epc band');
		});

		test('extracts pet policy from description', async () => {
			const html = loadFixture('987654321');
			const result = await scrapeListing('987654321', html);

			expect(result.success).toBe(true);
			if (!result.success) return;

			expect(result.listing.description).toContain('no pets');
		});
	});
});
