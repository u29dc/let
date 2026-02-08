/**
 * Fetch partial-failure tests
 *
 * Verifies:
 * 1. deduplicateListings resolves portal ID collisions correctly
 * 2. Fetch command emits ok: true even when all fetches fail (graceful partial failure)
 * 3. Existing data is preserved when fetch fails for a known portal ID
 */

import { afterAll, describe, expect, test } from 'bun:test';
import { mkdirSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import type { Listing } from '@let/core/schema';
import { deduplicateListings } from '../src/commands/shared-write.js';
import { run } from './harness.js';

// ---------------------------------------------------------------------------
// Unit tests: deduplicateListings
// ---------------------------------------------------------------------------

function makeListing(overrides: Partial<Listing> = {}): Listing {
	return {
		id: overrides.id ?? 'test-uuid',
		portalIds: overrides.portalIds ?? { rightmove: '100000001' },
		uprn: null,
		uprnSource: null,
		uprnConfidence: null,
		url: 'https://www.rightmove.co.uk/properties/100000001',
		location: { lat: 53.96, lng: -1.08, pinType: null },
		postcode: 'TE1 2ST',
		address: '42 Test Street, TestCity',
		googleMapsUrl: '',
		googleMapsStreetViewUrl: '',
		area: {
			lsoa: { code: null, name: null },
			msoa: { code: null, name: null },
			imd: { rank: null, decile: null, score: null },
			income: { bhc: null, ahc: null },
			socialHousingPct: null,
			population: null,
			floodRisk: { level: null, source: null },
			crime: { count12m: null, ratePer1k: null, violent12m: null, burglary12m: null, robbery12m: null, band: null, trend: null, updatedAt: null },
		},
		price: 1000,
		priceDisplay: '1,000 pcm',
		bedrooms: 2,
		bathrooms: 1,
		propertyType: 'terraced',
		description: '',
		notes: [],
		images: [],
		floorplan: { remote: null, local: null },
		epc: { remote: null, local: null },
		mapViews: { satellite: { remote: null, local: null }, street: { remote: null, local: null } },
		epcRating: null,
		floorAreaSqm: null,
		epcLodgementDate: null,
		epcAddressMatch: null,
		epcSearchUrl: '',
		nearestStations: [],
		gigabitAvailability: null,
		listedDate: null,
		lettings: { availableDate: null, deposit: null },
		agent: { name: null, phone: null },
		region: null,
		assessment: null,
		assessedAt: null,
		assessedScore: null,
		scores: null,
		fetchedAt: '2026-02-07T00:00:00Z',
		extractionStatus: 'success',
		status: 'active',
		...overrides,
	} as Listing;
}

describe('deduplicateListings', () => {
	test('removes duplicate portal IDs keeping newer version', () => {
		const older = makeListing({ id: 'uuid-old', portalIds: { rightmove: '12345' }, fetchedAt: '2026-02-01T00:00:00Z', price: 900 });
		const newer = makeListing({ id: 'uuid-new', portalIds: { rightmove: '12345' }, fetchedAt: '2026-02-08T00:00:00Z', price: 1100 });

		const { uniqueListings, removed, replaced } = deduplicateListings([older, newer]);

		expect(uniqueListings).toHaveLength(1);
		expect(removed).toBe(1);
		expect(replaced).toBe(1);
		expect(uniqueListings[0]?.price).toBe(1100);
	});

	test('carries over assessment from existing listing', () => {
		const assessment = {
			maintenance: 'good' as const,
			lightAndSpace: 'Bright',
			photoAnalysis: 'Good photos',
			recommendation: 'recommend' as const,
			familySuitability: 'good' as const,
			reasoning: 'Nice property',
			scoreAdjustment: 5,
		};

		const existing = makeListing({
			id: 'uuid-existing',
			portalIds: { rightmove: '12345' },
			fetchedAt: '2026-02-01T00:00:00Z',
			assessment,
			assessedAt: '2026-02-02T00:00:00Z',
			assessedScore: 75,
		});
		const incoming = makeListing({
			id: 'uuid-incoming',
			portalIds: { rightmove: '12345' },
			fetchedAt: '2026-02-08T00:00:00Z',
			assessment: null,
			assessedAt: null,
			assessedScore: null,
		});

		const { uniqueListings } = deduplicateListings([existing, incoming]);

		expect(uniqueListings).toHaveLength(1);
		const kept = uniqueListings[0];
		expect(kept?.assessment).toEqual(assessment);
		expect(kept?.assessedAt).toBe('2026-02-02T00:00:00Z');
		expect(kept?.id).toBe('uuid-existing');
	});

	test('preserves unique listings', () => {
		const a = makeListing({ id: 'uuid-a', portalIds: { rightmove: '11111' } });
		const b = makeListing({ id: 'uuid-b', portalIds: { rightmove: '22222' } });

		const { uniqueListings, removed } = deduplicateListings([a, b]);

		expect(uniqueListings).toHaveLength(2);
		expect(removed).toBe(0);
	});

	test('older duplicate does not replace existing', () => {
		const newer = makeListing({ id: 'uuid-newer', portalIds: { rightmove: '12345' }, fetchedAt: '2026-02-08T00:00:00Z', price: 1200 });
		const older = makeListing({ id: 'uuid-older', portalIds: { rightmove: '12345' }, fetchedAt: '2026-02-01T00:00:00Z', price: 900 });

		const { uniqueListings, removed, replaced } = deduplicateListings([newer, older]);

		expect(uniqueListings).toHaveLength(1);
		expect(removed).toBe(1);
		expect(replaced).toBe(0);
		expect(uniqueListings[0]?.price).toBe(1200);
	});
});

// ---------------------------------------------------------------------------
// Integration tests: fetch command graceful partial failure
// ---------------------------------------------------------------------------

const TEMP_DIR = join(tmpdir(), `let-fetch-partial-${Date.now()}-${Math.random().toString(36).slice(2)}`);
const DATA_DIR = TEMP_DIR;
const CACHE_DIR = join(TEMP_DIR, 'cache');
const SOURCES_DIR = join(TEMP_DIR, 'sources');
const DB_PATH = join(DATA_DIR, 'let.db');

mkdirSync(CACHE_DIR, { recursive: true });
mkdirSync(SOURCES_DIR, { recursive: true });

writeFileSync(
	join(DATA_DIR, 'let.config.toml'),
	`
[search]
locations = [{ id = "REGION^1234", name = "TestCity" }]
[search.filters]
minBedrooms = 2
maxBedrooms = 3
minPrice = 700
maxPrice = 1300
propertyTypes = ["terraced"]
includeLetAgreed = false
radius = 0
dontShow = []
mustHave = ["garden"]
[fetch]
delayMs = 100
maxListings = 100
maxRetries = 1
[scoring]
adaptiveness = 2.0
adaptivenessFactor = 10
[scoring.weights]
location = 0.40
affordability = 0.30
liveability = 0.30
[scoring.affordability]
priceWeight = 1.00
epcWeight = 0.00
[scoring.affordability.heatingCosts]
A = 30
B = 45
C = 70
D = 100
E = 400
F = 450
G = 500
[scoring.location]
priorityWeight = 0.30
broadbandWeight = 0.25
stationWeight = 0.25
imdWeight = 0.12
crimeWeight = 0.08
[scoring.liveability]
gardenWeight = 0.40
heatingWeight = 0.30
propertyTypeWeight = 0.30
[scoring.liveability.garden]
private = 100
shared = 40
none = 0
[scoring.liveability.heating]
gas = 100
electric = 60
unknown = 30
[scoring.liveability.propertyType]
terraced = 85
flat = 80
[scoring.penalties]
epcF = 0.00
epcG = 0.00
noGarden = 0.50
noPets = 0.90
missingDataPenalty = 0.95
[scoring.regionPriority]
TestCity = 85
`,
);

function seedDatabase() {
	const { Database } = require('bun:sqlite');
	const schema = require('node:fs').readFileSync(join(import.meta.dirname, '..', '..', 'core', 'src', 'db', 'schema.sql'), 'utf-8');
	const db = new Database(DB_PATH);
	db.run('PRAGMA foreign_keys = ON');
	db.exec(schema);
	db.run('INSERT INTO meta (id, updated_at, last_search_total) VALUES (1, ?, ?)', ['2026-02-08T00:00:00Z', 5]);
	db.run(
		`INSERT INTO listings (id, portal_rightmove, url, address, postcode, region, lat, lng, google_maps_url, google_maps_street_view_url, price, price_display, bedrooms, bathrooms, property_type, description, fetched_at, extraction_status, status) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)`,
		[
			'aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee',
			'170448131',
			'https://www.rightmove.co.uk/properties/170448131',
			'42 Test Street, TestCity',
			'TE1 2ST',
			'TestCity, TestCounty',
			53.96,
			-1.08,
			'https://maps.google.com/?q=53.96,-1.08',
			'https://maps.google.com/maps?layer=c&cbll=53.96,-1.08',
			1000,
			'1,000 pcm',
			3,
			1,
			'terraced',
			'A 3-bed terrace with garden.',
			'2026-02-08T00:00:00Z',
			'success',
			'active',
		],
	);
	db.run(
		`INSERT INTO scores (listing_id, overall, confidence, affordability, location, liveability, penalty_epc, penalty_garden, penalty_pets, penalty_combined, factor_monthly_rent, factor_price_percentile, factor_true_monthly_cost, factor_true_cost_percentile, factor_garden_type, factor_heating_type, factor_pet_policy, factor_bedrooms) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)`,
		['aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee', 72.5, 0.85, 68, 75, 70, 1, 1, 1, 1, 1000, 0.55, 1100, 0.5, 'private', 'gas', 'unknown', 3],
	);
	db.close();
}

seedDatabase();

afterAll(() => {
	try {
		rmSync(TEMP_DIR, { recursive: true, force: true });
	} catch {
		// ignore
	}
});

const ENV = {
	LET_DATA_DIR: DATA_DIR,
	LET_CONFIG_DIR: DATA_DIR,
	LET_CACHE_DIR: CACHE_DIR,
	LET_SOURCES_DIR: SOURCES_DIR,
	LET_JSON: '1',
};

describe('Fetch partial failure', () => {
	test('fetch with fake IDs returns ok: true with all in failed[]', async () => {
		const { stdout, exitCode } = await run(['fetch', 'FAKEID1,FAKEID2', '--skip-images', '--skip-epc', '--json'], ENV);
		const parsed = JSON.parse(stdout);

		expect(parsed['ok']).toBe(true);
		expect(exitCode).toBe(0);
		expect(parsed['data']['fetched']).toHaveLength(0);
		expect(parsed['data']['failed']).toHaveLength(2);
		expect(parsed['data']['total']).toBe(2);
		expect(parsed['meta']['tool']).toBe('fetch');
	});

	test('fetch with known portal ID preserves existing data on failure', async () => {
		const { stdout, exitCode } = await run(['fetch', '170448131', '--skip-images', '--skip-epc', '--json'], ENV);
		const parsed = JSON.parse(stdout);

		expect(parsed['ok']).toBe(true);
		expect(exitCode).toBe(0);

		// Verify the existing listing is still in the DB
		const { stdout: listStdout } = await run(['view', 'list', '--json'], ENV);
		const listParsed = JSON.parse(listStdout);
		expect(listParsed['ok']).toBe(true);
		expect(listParsed['data']['listings'].length).toBeGreaterThanOrEqual(1);
	});
});
