/**
 * LET-039: Contract tests for JSON envelope across commands
 *
 * Verifies stdout purity and envelope shape for all safe (non-network) commands.
 * Each command is tested with --json to ensure:
 * 1. stdout contains exactly one JSON line
 * 2. Envelope has { ok, data|error, meta } structure
 * 3. meta.tool matches expected tool name
 * 4. meta.elapsed is a non-negative number
 *
 * Uses in-process execution via harness for speed (~50ms vs ~850ms subprocess).
 */

import { afterAll, describe, expect, test } from 'bun:test';
import { mkdirSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { run } from './harness.js';

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
	LET_DATA_DIR: DATA_DIR,
	LET_CONFIG_DIR: DATA_DIR,
	LET_CACHE_DIR: CACHE_DIR,
	LET_SOURCES_DIR: SOURCES_DIR,
	LET_JSON: '1',
};

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

	return parsed;
}

// ---------------------------------------------------------------------------
// Contract tests
// ---------------------------------------------------------------------------

describe('JSON envelope contracts', () => {
	test('tools --json', async () => {
		const { stdout } = await run(['tools', '--json'], ENV);
		const parsed = assertValidEnvelope(stdout, 'tools');
		expect(parsed['ok']).toBe(true);
		expect(Array.isArray(parsed['data']['tools'])).toBe(true);
	});

	test('health --json', async () => {
		const { stdout } = await run(['health', '--json'], ENV);
		const parsed = assertValidEnvelope(stdout, 'health');
		expect(parsed['ok']).toBe(true);
	});

	test('config show --json', async () => {
		const { stdout } = await run(['config', 'show', '--json'], ENV);
		const parsed = assertValidEnvelope(stdout, 'config.show');
		expect(parsed['ok']).toBe(true);
	});

	test('config validate --json', async () => {
		const { stdout } = await run(['config', 'validate', '--json'], ENV);
		const parsed = assertValidEnvelope(stdout, 'config.validate');
		expect(parsed['ok']).toBe(true);
	});

	test('view list --json', async () => {
		const { stdout } = await run(['view', 'list', '--json'], ENV);
		const parsed = assertValidEnvelope(stdout, 'view.list');
		expect(parsed['ok']).toBe(true);
	});

	test('view detail --json', async () => {
		const { stdout } = await run(['view', 'detail', LISTING_ID, '--json'], ENV);
		const parsed = assertValidEnvelope(stdout, 'view.detail');
		expect(parsed['ok']).toBe(true);
	});

	test('view detail --json (not found)', async () => {
		const { stdout, exitCode } = await run(['view', 'detail', '00000000-0000-0000-0000-000000000000', '--json'], ENV);
		const parsed = assertValidEnvelope(stdout, 'view.detail');
		expect(parsed['ok']).toBe(false);
		expect(parsed['error']['code']).toBe('NOT_FOUND');
		expect(exitCode).toBe(1);
	});

	test('score explain --json', async () => {
		const { stdout } = await run(['score', 'explain', LISTING_ID, '--json'], ENV);
		const parsed = assertValidEnvelope(stdout, 'score.explain');
		expect(parsed['ok']).toBe(true);
	});

	test('score compute --json', async () => {
		const { stdout } = await run(['score', 'compute', '--json'], ENV);
		const parsed = assertValidEnvelope(stdout, 'score.compute');
		expect(parsed['ok']).toBe(true);
	});

	test('assess candidates --json', async () => {
		const { stdout } = await run(['assess', 'candidates', '--json'], ENV);
		const parsed = assertValidEnvelope(stdout, 'assess.candidates');
		expect(parsed['ok']).toBe(true);
	});

	test('assess context --json', async () => {
		const { stdout } = await run(['assess', 'context', LISTING_ID, '--json'], ENV);
		const parsed = assertValidEnvelope(stdout, 'assess.context');
		expect(parsed['ok']).toBe(true);
	});

	test('search diff --json', async () => {
		const { stdout } = await run(['search', 'diff', '170448131,999999999', '--json'], ENV);
		const parsed = assertValidEnvelope(stdout, 'search.diff');
		expect(parsed['ok']).toBe(true);
	});

	test('ops prune --inactive --json (no inactive)', async () => {
		const { stdout } = await run(['ops', 'prune', '--inactive', '--json'], ENV);
		const parsed = assertValidEnvelope(stdout, 'ops.prune');
		expect(parsed['ok']).toBe(true);
		const data = parsed['data'] as Record<string, unknown>;
		expect(data['removed']).toBe(0);
		expect(data['mode']).toBe('inactive');
	});

	test('ops prune --min-score 100 --dry-run --json', async () => {
		const { stdout } = await run(['ops', 'prune', '--min-score', '100', '--dry-run', '--json'], ENV);
		const parsed = assertValidEnvelope(stdout, 'ops.prune');
		expect(parsed['ok']).toBe(true);
		const data = parsed['data'] as Record<string, unknown>;
		expect(data['dryRun']).toBe(true);
		expect(data['removed']).toBe(1);
	});

	test('export json --json', async () => {
		const outputPath = join(TEMP_DIR, 'test-export.json');
		const { stdout } = await run(['export', 'json', '--output', outputPath, '--json'], ENV);
		const parsed = assertValidEnvelope(stdout, 'export.json');
		expect(parsed['ok']).toBe(true);
		expect(parsed['data']['count']).toBe(1);
	});
});

// ---------------------------------------------------------------------------
// Assessment atomicity regression test
// ---------------------------------------------------------------------------

describe('Assessment atomicity', () => {
	const ASSESSMENT_ID = 'aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee';

	test('sequential assessments both persist (no data loss from targeted UPDATE)', async () => {
		// First assessment
		const assessment1 = JSON.stringify({
			maintenance: 'good',
			lightAndSpace: 'Bright rooms with south-facing windows',
			photoAnalysis: 'Photos show well-maintained property',
			recommendation: 'recommend',
			familySuitability: 'good',
			reasoning: 'Good value terrace with garden',
			scoreAdjustment: 5,
		});
		const { stdout: stdout1 } = await run(['assess', 'submit', ASSESSMENT_ID, assessment1, '--json'], ENV);
		const parsed1 = JSON.parse(stdout1);
		expect(parsed1['ok']).toBe(true);
		expect(parsed1['data']['scoreAdjustment']).toBe(5);

		// Second assessment overwrites the first (same listing)
		const assessment2 = JSON.stringify({
			maintenance: 'excellent',
			lightAndSpace: 'Very bright with large windows',
			photoAnalysis: 'Excellent condition throughout',
			recommendation: 'strong-recommend',
			familySuitability: 'excellent',
			reasoning: 'Outstanding property after closer inspection',
			scoreAdjustment: 10,
		});
		const { stdout: stdout2 } = await run(['assess', 'submit', ASSESSMENT_ID, assessment2, '--json'], ENV);
		const parsed2 = JSON.parse(stdout2);
		expect(parsed2['ok']).toBe(true);
		expect(parsed2['data']['scoreAdjustment']).toBe(10);

		// Verify latest assessment persisted via view detail
		const { stdout: detailStdout } = await run(['view', 'detail', ASSESSMENT_ID, '--json'], ENV);
		const detail = JSON.parse(detailStdout);
		expect(detail['ok']).toBe(true);
		const listing = detail['data']['listing'] as Record<string, unknown>;
		expect(listing['assessedScore']).toBeDefined();
		const assessment = listing['assessment'] as Record<string, unknown>;
		expect(assessment['maintenance']).toBe('excellent');
		expect(assessment['scoreAdjustment']).toBe(10);
	});
});

// ---------------------------------------------------------------------------
// Search fixture tests (LET-030)
// ---------------------------------------------------------------------------

describe('Search fixture tests', () => {
	test('search diff classifies unknown IDs as new', async () => {
		const { stdout } = await run(['search', 'diff', '111111111,222222222,333333333', '--json'], ENV);
		const parsed = assertValidEnvelope(stdout, 'search.diff');
		expect(parsed['ok']).toBe(true);
		const data = parsed['data'] as Record<string, unknown>;
		expect((data['new'] as string[]).length).toBe(3);
		expect((data['known'] as string[]).length).toBe(0);
	});
});

// ---------------------------------------------------------------------------
// Registry drift test (LET-009)
// ---------------------------------------------------------------------------

describe('Registry drift', () => {
	test('tools catalog has exactly 18 registered tools', async () => {
		const { stdout } = await run(['tools', '--json'], ENV);
		const parsed = JSON.parse(stdout);
		expect(parsed['data']['tools'].length).toBe(18);
	});

	test('every registered tool name matches a routable command', async () => {
		const { stdout } = await run(['tools', '--json'], ENV);
		const parsed = JSON.parse(stdout);
		const names: string[] = parsed['data']['tools'].map((t: { name: string }) => t.name);

		const expected = [
			'assess.candidates',
			'assess.context',
			'assess.submit',
			'config.show',
			'config.validate',
			'export.json',
			'export.notion',
			'fetch',
			'ops.patch',
			'ops.prune',
			'ops.verify',
			'score.compute',
			'score.explain',
			'search.diff',
			'search.discover',
			'search.resolve',
			'view.detail',
			'view.list',
		];

		expect(names.sort()).toEqual(expected.sort());
	});

	test('no legacy tool names in catalog', async () => {
		const { stdout } = await run(['tools', '--json'], ENV);
		const parsed = JSON.parse(stdout);
		const names: string[] = parsed['data']['tools'].map((t: { name: string }) => t.name);

		const legacy = ['help', 'output', 'output.json', 'output.notion', 'ops.enrich', 'view.stats', 'view.regions'];
		for (const name of legacy) {
			expect(names).not.toContain(name);
		}
	});

	test('fetch tool has --region parameter', async () => {
		const { stdout } = await run(['tools', '--json'], ENV);
		const parsed = JSON.parse(stdout);
		const fetchTool = parsed['data']['tools'].find((t: { name: string }) => t.name === 'fetch');
		const paramNames: string[] = fetchTool['parameters'].map((p: { name: string }) => p.name);
		expect(paramNames).toContain('--region');
	});

	test('search.discover tool has --location, --property-types params and idsByLocation output', async () => {
		const { stdout } = await run(['tools', '--json'], ENV);
		const parsed = JSON.parse(stdout);
		const discoverTool = parsed['data']['tools'].find((t: { name: string }) => t.name === 'search.discover');
		const paramNames: string[] = discoverTool['parameters'].map((p: { name: string }) => p.name);
		expect(paramNames).toContain('--location');
		expect(paramNames).toContain('--property-types');
		const outputFields: string[] = discoverTool['outputFields'];
		expect(outputFields).toContain('idsByLocation');
	});

	test('assess.submit has inputSchema in catalog', async () => {
		const { stdout } = await run(['tools', '--json'], ENV);
		const parsed = JSON.parse(stdout);
		const submit = parsed['data']['tools'].find((t: { name: string }) => t.name === 'assess.submit');
		expect(submit['inputSchema']).toBeDefined();
		expect(submit['inputSchema']['required']).toContain('maintenance');
	});

	test('view.list has outputSchema in catalog', async () => {
		const { stdout } = await run(['tools', '--json'], ENV);
		const parsed = JSON.parse(stdout);
		const viewList = parsed['data']['tools'].find((t: { name: string }) => t.name === 'view.list');
		expect(viewList['outputSchema']).toBeDefined();
		expect(viewList['outputSchema']['listings']).toHaveProperty('type', 'array');
	});

	test('tools and health are infrastructure, not in catalog', async () => {
		const { stdout } = await run(['tools', '--json'], ENV);
		const parsed = JSON.parse(stdout);
		const names: string[] = parsed['data']['tools'].map((t: { name: string }) => t.name);

		expect(names).not.toContain('tools');
		expect(names).not.toContain('health');
	});
});
