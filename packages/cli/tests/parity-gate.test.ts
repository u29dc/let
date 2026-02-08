/**
 * LET-035: Parity gate test
 *
 * Demonstrates end-to-end loop using only new commands:
 *   tools → health → config show → search diff → view list →
 *   score explain → score compute → assess context → assess submit → view detail
 *
 * Network-dependent commands (search discover, fetch) are excluded.
 * Uses a seeded SQLite database in a temp directory.
 */

import { afterAll, describe, expect, test } from 'bun:test';
import { mkdirSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const CLI_ENTRY = join(import.meta.dirname, '..', 'src', 'index.ts');

// ---------------------------------------------------------------------------
// Temp dir setup
// ---------------------------------------------------------------------------

const TEMP_DIR = join(tmpdir(), `let-parity-${Date.now()}-${Math.random().toString(36).slice(2)}`);
const DATA_DIR = join(TEMP_DIR, 'data');
const CACHE_DIR = join(TEMP_DIR, 'cache');
const SOURCES_DIR = join(TEMP_DIR, 'sources');
const CONFIG_DIR = DATA_DIR;
const DB_PATH = join(DATA_DIR, 'let.db');

mkdirSync(DATA_DIR, { recursive: true });
mkdirSync(CACHE_DIR, { recursive: true });
mkdirSync(SOURCES_DIR, { recursive: true });

// Write a minimal config
const CONFIG_TOML = `
[search]
locations = [
    { id = "REGION^1234", name = "TestCity" },
]

[search.filters]
minBedrooms = 2
maxBedrooms = 3
minPrice = 700
maxPrice = 1300
propertyTypes = ["detached", "semi-detached", "terraced"]
includeLetAgreed = false
radius = 0
dontShow = ["houseShare"]
mustHave = ["garden"]

[fetch]
useApi = false
delayMs = 1000
maxListings = 100
maxRetries = 3

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
detached = 95
semi-detached = 90
terraced = 85
flat = 80
cottage = 70
bungalow = 60
studio = 40

[scoring.penalties]
epcF = 0.00
epcG = 0.00
noGarden = 0.50
noPets = 0.90
missingDataPenalty = 0.95

[scoring.regionPriority]
TestCity = 85
`;

writeFileSync(join(CONFIG_DIR, 'let.config.toml'), CONFIG_TOML);

// Seed a SQLite database with one listing
function seedDatabase() {
	const { Database } = require('bun:sqlite');
	const schemaPath = join(import.meta.dirname, '..', '..', 'core', 'src', 'db', 'schema.sql');
	const schema = require('node:fs').readFileSync(schemaPath, 'utf-8');

	const db = new Database(DB_PATH);
	db.run('PRAGMA foreign_keys = ON');
	db.exec(schema);

	db.run('INSERT INTO meta (id, updated_at, last_search_total) VALUES (1, ?, ?)', ['2026-02-08T00:00:00Z', 5]);
	db.run('INSERT INTO search_urls (url) VALUES (?)', ['https://example.com/search']);
	db.run('INSERT INTO search_locations (name) VALUES (?)', ['TestCity']);

	db.run(
		`INSERT INTO listings (
			id, portal_rightmove, url, address, postcode, region,
			lat, lng, google_maps_url, google_maps_street_view_url,
			price, price_display, bedrooms, bathrooms, property_type,
			description, fetched_at, extraction_status, status
		) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)`,
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
			'£1,000 pcm',
			3,
			1,
			'terraced',
			'A lovely 3-bed terrace with garden and gas central heating.',
			'2026-02-08T00:00:00Z',
			'success',
			'active',
		],
	);

	db.run(`INSERT INTO images (listing_id, remote, local, position) VALUES (?, ?, ?, ?)`, ['aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee', 'https://media.rightmove.co.uk/photo1.jpg', null, 0]);

	db.run(`INSERT INTO stations (listing_id, name, distance, unit, position) VALUES (?, ?, ?, ?, ?)`, ['aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee', 'TestCity Station', 0.3, 'miles', 0]);

	db.run(`INSERT INTO notes (listing_id, note, position) VALUES (?, ?, ?)`, ['aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee', 'Has garden', 0]);

	db.run(
		`INSERT INTO scores (
			listing_id, overall, confidence, affordability, location, liveability,
			penalty_epc, penalty_garden, penalty_pets, penalty_combined,
			factor_monthly_rent, factor_price_percentile,
			factor_true_monthly_cost, factor_true_cost_percentile,
			factor_garden_type, factor_heating_type, factor_pet_policy, factor_bedrooms
		) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)`,
		['aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee', 72.5, 0.85, 68.0, 75.0, 70.0, 1.0, 1.0, 1.0, 1.0, 1000, 0.55, 1100, 0.5, 'private', 'gas', 'unknown', 3],
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

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

const ENV = {
	...process.env,
	LET_DATA_DIR: DATA_DIR,
	LET_CONFIG_DIR: CONFIG_DIR,
	LET_CACHE_DIR: CACHE_DIR,
	LET_SOURCES_DIR: SOURCES_DIR,
	LET_JSON: '1',
};

function run(args: string[]): { stdout: string; stderr: string; exitCode: number } {
	const result = Bun.spawnSync(['bun', 'run', CLI_ENTRY, ...args], { env: ENV });
	return {
		stdout: result.stdout.toString().trim(),
		stderr: result.stderr.toString().trim(),
		exitCode: result.exitCode,
	};
}

function parseJson(stdout: string): Record<string, unknown> {
	return JSON.parse(stdout);
}

// ---------------------------------------------------------------------------
// Parity gate chain
// ---------------------------------------------------------------------------

const LISTING_ID = 'aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee';
const PORTAL_ID = '170448131';

describe('parity gate: end-to-end with new commands', () => {
	test('1. tools --json → returns tool catalog', () => {
		const { stdout } = run(['tools', '--json']);
		const parsed = parseJson(stdout);
		expect(parsed['ok']).toBe(true);
		expect(parsed['meta']).toHaveProperty('tool', 'tools');
		const data = parsed['data'] as Record<string, unknown>;
		expect(Array.isArray(data['tools'])).toBe(true);
		expect((data['tools'] as unknown[]).length).toBeGreaterThan(0);
	});

	test('2. health --json → returns health status', () => {
		const { stdout } = run(['health', '--json']);
		const parsed = parseJson(stdout);
		expect(parsed['ok']).toBe(true);
		const data = parsed['data'] as Record<string, unknown>;
		expect(data['status']).toMatch(/^(ready|degraded|blocked)$/);
		expect(data['paths']).toBeDefined();
		expect(Array.isArray(data['checks'])).toBe(true);
	});

	test('3. config show --json → returns config', () => {
		const { stdout } = run(['config', 'show', '--json']);
		const parsed = parseJson(stdout);
		expect(parsed['ok']).toBe(true);
		const data = parsed['data'] as Record<string, unknown>;
		const config = data['config'] as Record<string, unknown>;
		expect(config).toHaveProperty('search');
		expect(config).toHaveProperty('scoring');
		expect(config).toHaveProperty('fetch');
	});

	test('4. config validate --json → config is valid', () => {
		const { stdout, exitCode } = run(['config', 'validate', '--json']);
		const parsed = parseJson(stdout);
		expect(parsed['ok']).toBe(true);
		const data = parsed['data'] as Record<string, unknown>;
		expect(data['valid']).toBe(true);
		expect(exitCode).toBe(0);
	});

	test('5. search diff --json → classifies IDs as new/known', () => {
		const { stdout } = run(['search', 'diff', `${PORTAL_ID},999999999`, '--json']);
		const parsed = parseJson(stdout);
		expect(parsed['ok']).toBe(true);
		const data = parsed['data'] as Record<string, unknown>;
		expect(Array.isArray(data['new'])).toBe(true);
		expect(Array.isArray(data['known'])).toBe(true);
		// 170448131 is known (in DB), 999999999 is new
		expect(data['known']).toContain(PORTAL_ID);
		expect(data['new']).toContain('999999999');
	});

	test('6. view list --json → returns ranked listings', () => {
		const { stdout } = run(['view', 'list', '--json']);
		const parsed = parseJson(stdout);
		expect(parsed['ok']).toBe(true);
		const data = parsed['data'] as Record<string, unknown>;
		expect(data['total']).toBe(1);
		expect(Array.isArray(data['listings'])).toBe(true);
		const listings = data['listings'] as Record<string, unknown>[];
		expect(listings.length).toBe(1);
		expect(listings[0]).toHaveProperty('id');
		expect(listings[0]).toHaveProperty('score');
	});

	test('7. view detail --json → returns full listing', () => {
		const { stdout } = run(['view', 'detail', LISTING_ID, '--json']);
		const parsed = parseJson(stdout);
		expect(parsed['ok']).toBe(true);
		const data = parsed['data'] as Record<string, unknown>;
		const listing = data['listing'] as Record<string, unknown>;
		expect(listing['id']).toBe(LISTING_ID);
		expect(listing['address']).toBe('42 Test Street, TestCity');
		expect(listing['price']).toBe(1000);
	});

	test('8. score explain --json → returns score breakdown', () => {
		const { stdout } = run(['score', 'explain', LISTING_ID, '--json']);
		const parsed = parseJson(stdout);
		expect(parsed['ok']).toBe(true);
		const data = parsed['data'] as Record<string, unknown>;
		expect(data).toHaveProperty('overall');
		expect(data).toHaveProperty('composites');
		expect(data).toHaveProperty('penalties');
	});

	test('9. score compute --json → rescores all listings', () => {
		const { stdout } = run(['score', 'compute', '--json']);
		const parsed = parseJson(stdout);
		expect(parsed['ok']).toBe(true);
		const data = parsed['data'] as Record<string, unknown>;
		expect(data['total']).toBe(1);
		expect(data['scored']).toBe(1);
		expect(typeof data['avgScore']).toBe('number');
	});

	test('10. assess candidates --json → returns unassessed listings', () => {
		const { stdout } = run(['assess', 'candidates', '--json']);
		const parsed = parseJson(stdout);
		expect(parsed['ok']).toBe(true);
		const data = parsed['data'] as Record<string, unknown>;
		expect(Array.isArray(data['candidates'])).toBe(true);
		// Our listing has no assessment, so it should be a candidate
		const candidates = data['candidates'] as Record<string, unknown>[];
		expect(candidates.length).toBe(1);
	});

	test('11. assess context --json → returns assessment bundle', () => {
		const { stdout } = run(['assess', 'context', LISTING_ID, '--json']);
		const parsed = parseJson(stdout);
		expect(parsed['ok']).toBe(true);
		const data = parsed['data'] as Record<string, unknown>;
		expect(data).toHaveProperty('listing');
		expect(data).toHaveProperty('scoreBreakdown');
		expect(data).toHaveProperty('assessmentSchema');
	});

	test('12. assess submit --json → saves assessment', () => {
		const assessment = JSON.stringify({
			maintenance: 'good',
			lightAndSpace: 'Bright rooms with south-facing windows',
			photoAnalysis: 'Photos show well-maintained property',
			recommendation: 'recommend',
			familySuitability: 'good',
			reasoning: 'Good value 3-bed terrace with garden in TestCity',
			scoreAdjustment: 3,
		});
		const { stdout, exitCode } = run(['assess', 'submit', LISTING_ID, assessment, '--json']);
		const parsed = parseJson(stdout);
		expect(parsed['ok']).toBe(true);
		const data = parsed['data'] as Record<string, unknown>;
		expect(data['id']).toBe(LISTING_ID);
		expect(typeof data['assessedScore']).toBe('number');
		expect(data['scoreAdjustment']).toBe(3);
		expect(exitCode).toBe(0);
	});

	test('13. view list --json (post-assess) → shows assessed score', () => {
		const { stdout } = run(['view', 'list', '--json']);
		const parsed = parseJson(stdout);
		expect(parsed['ok']).toBe(true);
		const data = parsed['data'] as Record<string, unknown>;
		const listings = data['listings'] as Record<string, unknown>[];
		expect(listings.length).toBe(1);
		// After assess submit, assessedScore should be set
		expect(listings[0]?.['assessedScore']).toBeDefined();
	});
});
