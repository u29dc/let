/**
 * LET-039: Contract tests for JSON envelope across commands
 *
 * Verifies stdout purity and envelope shape for all safe (non-network) commands.
 * Each command is tested with --json and LET_JSON=1 to ensure:
 * 1. stdout contains exactly one JSON line
 * 2. Envelope has { ok, data|error, meta } structure
 * 3. meta.tool matches expected tool name
 * 4. meta.elapsed is a non-negative number
 */

import { afterAll, describe, expect, test } from 'bun:test';
import { mkdirSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const CLI_ENTRY = join(import.meta.dirname, '..', 'src', 'index.ts');

// ---------------------------------------------------------------------------
// Fixture setup
// ---------------------------------------------------------------------------

const TEMP_DIR = join(tmpdir(), `let-contract-${Date.now()}-${Math.random().toString(36).slice(2)}`);
const DATA_DIR = TEMP_DIR;
const CACHE_DIR = join(TEMP_DIR, 'cache');
const SOURCES_DIR = join(TEMP_DIR, 'sources');
const DB_PATH = join(DATA_DIR, 'let.db');
const LISTING_ID = 'aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee';

mkdirSync(CACHE_DIR, { recursive: true });
mkdirSync(SOURCES_DIR, { recursive: true });

// Write config
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

// Seed database
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
			LISTING_ID,
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
			'A 3-bed terrace with garden.',
			'2026-02-08T00:00:00Z',
			'success',
			'active',
		],
	);
	db.run(
		`INSERT INTO scores (listing_id, overall, confidence, affordability, location, liveability, penalty_epc, penalty_garden, penalty_pets, penalty_combined, factor_monthly_rent, factor_price_percentile, factor_true_monthly_cost, factor_true_cost_percentile, factor_garden_type, factor_heating_type, factor_pet_policy, factor_bedrooms) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)`,
		[LISTING_ID, 72.5, 0.85, 68, 75, 70, 1, 1, 1, 1, 1000, 0.55, 1100, 0.5, 'private', 'gas', 'unknown', 3],
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
// Test helpers
// ---------------------------------------------------------------------------

const ENV = {
	...process.env,
	LET_DATA_DIR: DATA_DIR,
	LET_CONFIG_DIR: DATA_DIR,
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

/** Validate JSON envelope structure */
function assertValidEnvelope(stdout: string, expectedTool: string) {
	// Must parse as valid JSON
	const parsed = JSON.parse(stdout);

	// Must have ok field
	expect(typeof parsed['ok']).toBe('boolean');

	// Must have meta with tool and elapsed
	expect(parsed['meta']).toBeDefined();
	expect(parsed['meta']['tool']).toBe(expectedTool);
	expect(typeof parsed['meta']['elapsed']).toBe('number');
	expect(parsed['meta']['elapsed']).toBeGreaterThanOrEqual(0);

	if (parsed['ok'] === true) {
		// Success: must have data
		expect(parsed['data']).toBeDefined();
	} else {
		// Error: must have error with code, message, hint
		expect(parsed['error']).toBeDefined();
		expect(typeof parsed['error']['code']).toBe('string');
		expect(typeof parsed['error']['message']).toBe('string');
		expect(typeof parsed['error']['hint']).toBe('string');
	}

	// stdout must be exactly one JSON line (no extra bytes)
	const lines = stdout.split('\n').filter(Boolean);
	expect(lines.length).toBe(1);

	return parsed;
}

// ---------------------------------------------------------------------------
// Contract tests
// ---------------------------------------------------------------------------

describe('JSON envelope contracts', () => {
	test('tools --json', () => {
		const { stdout } = run(['tools', '--json']);
		const parsed = assertValidEnvelope(stdout, 'tools');
		expect(parsed['ok']).toBe(true);
		expect(Array.isArray(parsed['data']['tools'])).toBe(true);
	});

	test('health --json', () => {
		const { stdout } = run(['health', '--json']);
		const parsed = assertValidEnvelope(stdout, 'health');
		expect(parsed['ok']).toBe(true);
	});

	test('config show --json', () => {
		const { stdout } = run(['config', 'show', '--json']);
		const parsed = assertValidEnvelope(stdout, 'config.show');
		expect(parsed['ok']).toBe(true);
	});

	test('config validate --json', () => {
		const { stdout } = run(['config', 'validate', '--json']);
		const parsed = assertValidEnvelope(stdout, 'config.validate');
		expect(parsed['ok']).toBe(true);
	});

	test('view list --json', () => {
		const { stdout } = run(['view', 'list', '--json']);
		const parsed = assertValidEnvelope(stdout, 'view.list');
		expect(parsed['ok']).toBe(true);
	});

	test('view detail --json', () => {
		const { stdout } = run(['view', 'detail', LISTING_ID, '--json']);
		const parsed = assertValidEnvelope(stdout, 'view.detail');
		expect(parsed['ok']).toBe(true);
	});

	test('view detail --json (not found)', () => {
		const { stdout, exitCode } = run(['view', 'detail', '00000000-0000-0000-0000-000000000000', '--json']);
		const parsed = assertValidEnvelope(stdout, 'view.detail');
		expect(parsed['ok']).toBe(false);
		expect(parsed['error']['code']).toBe('NOT_FOUND');
		expect(exitCode).toBe(1);
	});

	test('score explain --json', () => {
		const { stdout } = run(['score', 'explain', LISTING_ID, '--json']);
		const parsed = assertValidEnvelope(stdout, 'score.explain');
		expect(parsed['ok']).toBe(true);
	});

	test('score compute --json', () => {
		const { stdout } = run(['score', 'compute', '--json']);
		const parsed = assertValidEnvelope(stdout, 'score.compute');
		expect(parsed['ok']).toBe(true);
	});

	test('assess candidates --json', () => {
		const { stdout } = run(['assess', 'candidates', '--json']);
		const parsed = assertValidEnvelope(stdout, 'assess.candidates');
		expect(parsed['ok']).toBe(true);
	});

	test('assess context --json', () => {
		const { stdout } = run(['assess', 'context', LISTING_ID, '--json']);
		const parsed = assertValidEnvelope(stdout, 'assess.context');
		expect(parsed['ok']).toBe(true);
	});

	test('search diff --json', () => {
		const { stdout } = run(['search', 'diff', '170448131,999999999', '--json']);
		const parsed = assertValidEnvelope(stdout, 'search.diff');
		expect(parsed['ok']).toBe(true);
	});

	test('export json --json', () => {
		const outputPath = join(TEMP_DIR, 'test-export.json');
		const { stdout } = run(['export', 'json', '--output', outputPath, '--json']);
		const parsed = assertValidEnvelope(stdout, 'export.json');
		expect(parsed['ok']).toBe(true);
		expect(parsed['data']['count']).toBe(1);
	});
});
