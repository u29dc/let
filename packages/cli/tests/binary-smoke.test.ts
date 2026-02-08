/**
 * LET-043: Binary smoke tests
 *
 * Builds the CLI binary and verifies it works from an arbitrary directory.
 * Tests installed-binary mode path resolution (no monorepo structure).
 *
 * Covers:
 * 1. Binary exists after build
 * 2. `bin/let tools --json` returns valid envelope from arbitrary cwd
 * 3. `bin/let health --json` returns valid envelope from arbitrary cwd
 * 4. `bin/let config show --json` works with explicit LET_DATA_DIR
 * 5. JSON envelope structure is correct (ok, meta.tool, meta.elapsed)
 */

import { afterAll, beforeAll, describe, expect, test } from 'bun:test';
import { existsSync, mkdirSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const PROJECT_ROOT = join(import.meta.dirname, '..', '..', '..');
const BINARY_PATH = join(PROJECT_ROOT, 'bin', 'let');

// Arbitrary temp directory to run from (NOT inside the monorepo)
const WORK_DIR = join(tmpdir(), `let-smoke-${Date.now()}-${Math.random().toString(36).slice(2)}`);
const DATA_DIR = join(WORK_DIR, 'data');

// ---------------------------------------------------------------------------
// Setup: build binary + create temp fixture
// ---------------------------------------------------------------------------

beforeAll(() => {
	// Build binary
	const build = Bun.spawnSync(['bun', 'run', 'build:cli'], { cwd: PROJECT_ROOT });
	if (build.exitCode !== 0) {
		throw new Error(`Binary build failed: ${build.stderr.toString()}`);
	}

	// Create temp directory with minimal config
	// In installed (non-dev) mode, config file is named config.toml
	mkdirSync(DATA_DIR, { recursive: true });
	writeFileSync(
		join(DATA_DIR, 'config.toml'),
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
mustHave = []
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
});

afterAll(() => {
	try {
		rmSync(WORK_DIR, { recursive: true, force: true });
	} catch {
		// ignore
	}
});

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function runBinary(args: string[], env?: Record<string, string>): { stdout: string; stderr: string; exitCode: number } {
	const result = Bun.spawnSync([BINARY_PATH, ...args], {
		cwd: WORK_DIR,
		env: { ...process.env, ...env },
	});
	return {
		stdout: result.stdout.toString().trim(),
		stderr: result.stderr.toString().trim(),
		exitCode: result.exitCode,
	};
}

function assertValidEnvelope(stdout: string, expectedTool: string) {
	const parsed = JSON.parse(stdout);
	expect(typeof parsed['ok']).toBe('boolean');
	expect(parsed['meta']).toBeDefined();
	expect(parsed['meta']['tool']).toBe(expectedTool);
	expect(typeof parsed['meta']['elapsed']).toBe('number');
	expect(parsed['meta']['elapsed']).toBeGreaterThanOrEqual(0);

	// stdout must be exactly one JSON line
	const lines = stdout.split('\n').filter(Boolean);
	expect(lines.length).toBe(1);

	return parsed;
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

describe('Binary smoke tests', () => {
	test('binary exists', () => {
		expect(existsSync(BINARY_PATH)).toBe(true);
	});

	// Single tools --json invocation covering all tool catalog assertions
	test('tools --json from arbitrary cwd', () => {
		const { stdout, exitCode } = runBinary(['tools', '--json']);
		expect(exitCode).toBe(0);
		const parsed = assertValidEnvelope(stdout, 'tools');
		expect(parsed['ok']).toBe(true);
		const toolNames: string[] = parsed['data']['tools'].map((t: { name: string }) => t.name);
		expect(toolNames.length).toBeGreaterThan(0);

		// Expected commands registered via defineToolCommand
		const expected = ['config.show', 'config.validate', 'view.list', 'view.detail', 'fetch', 'score.explain', 'score.compute'];
		for (const name of expected) {
			expect(toolNames).toContain(name);
		}

		// Legacy names that should NOT appear
		const legacy = ['help', 'output', 'output.json', 'output.notion', 'ops.enrich'];
		for (const name of legacy) {
			expect(toolNames).not.toContain(name);
		}
	});

	test('health --json from arbitrary cwd', () => {
		const { stdout } = runBinary(['health', '--json']);
		const parsed = assertValidEnvelope(stdout, 'health');
		expect(parsed['ok']).toBe(true);
		expect(parsed['data']['status']).toBeDefined();
	});

	test('config show --json with LET_DATA_DIR', () => {
		const { stdout, exitCode } = runBinary(['config', 'show', '--json'], {
			LET_DATA_DIR: DATA_DIR,
			LET_CONFIG_DIR: DATA_DIR,
		});
		expect(exitCode).toBe(0);
		const parsed = assertValidEnvelope(stdout, 'config.show');
		expect(parsed['ok']).toBe(true);
		expect(parsed['data']['config']).toBeDefined();
	});

	test('config validate --json with LET_DATA_DIR', () => {
		const { stdout, exitCode } = runBinary(['config', 'validate', '--json'], {
			LET_DATA_DIR: DATA_DIR,
			LET_CONFIG_DIR: DATA_DIR,
		});
		expect(exitCode).toBe(0);
		const parsed = assertValidEnvelope(stdout, 'config.validate');
		expect(parsed['ok']).toBe(true);
	});
});
