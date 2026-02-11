/**
 * Tests for cross-platform path resolution.
 *
 * Uses resetPaths() between tests to avoid cache leaking.
 * Sets LET_HOME to a temp dir for deterministic defaults.
 */

import { afterEach, beforeEach, describe, expect, test } from 'bun:test';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { paths, resetPaths, resolvePaths } from '../src/paths.js';

/** Save LET_HOME from the parent environment so we can restore it */
const savedLET_HOME = process.env['LET_HOME'];

/** Helper: clear all LET_* env vars to get a clean slate */
function clearEnv(): void {
	delete process.env['LET_HOME'];
	delete process.env['TOOLS_HOME'];
	delete process.env['LET_DATA_DIR'];
	delete process.env['LET_CONFIG_DIR'];
	delete process.env['LET_CACHE_DIR'];
	delete process.env['LET_SOURCES_DIR'];
}

const TEST_HOME = join(tmpdir(), 'let-test-home');

beforeEach(() => {
	resetPaths();
	clearEnv();
});

afterEach(() => {
	resetPaths();
	clearEnv();
	// Restore LET_HOME for other test files in the same process
	if (savedLET_HOME) process.env['LET_HOME'] = savedLET_HOME;
});

describe('paths', () => {
	describe('default resolution', () => {
		test('uses LET_HOME when set', () => {
			process.env['LET_HOME'] = TEST_HOME;
			const { resolved } = resolvePaths();
			expect(resolved.data).toBe(join(TEST_HOME, 'data'));
			expect(resolved.cache).toBe(join(TEST_HOME, 'cache'));
			expect(resolved.sources).toBe(join(TEST_HOME, 'sources'));
		});

		test('uses TOOLS_HOME/let when LET_HOME is not set', () => {
			const toolsHome = join(tmpdir(), 'test-tools');
			process.env['TOOLS_HOME'] = toolsHome;
			const { resolved } = resolvePaths();
			expect(resolved.data).toBe(join(toolsHome, 'let', 'data'));
			expect(resolved.cache).toBe(join(toolsHome, 'let', 'cache'));
			expect(resolved.sources).toBe(join(toolsHome, 'let', 'sources'));
		});

		test('falls back to ~/.tools/let when no env vars set', () => {
			const { resolved } = resolvePaths();
			expect(resolved.data).toMatch(/\.tools\/let\/data$/);
			expect(resolved.cache).toMatch(/\.tools\/let\/cache$/);
			expect(resolved.sources).toMatch(/\.tools\/let\/sources$/);
		});
	});

	describe('derived paths', () => {
		test('configFile is always let.config.toml', () => {
			process.env['LET_HOME'] = TEST_HOME;
			const { derived, resolved } = resolvePaths();
			expect(derived.configFile).toBe(join(resolved.config, 'let.config.toml'));
		});

		test('database path is under data dir', () => {
			process.env['LET_HOME'] = TEST_HOME;
			const { derived, resolved } = resolvePaths();
			expect(derived.database).toBe(join(resolved.data, 'let.db'));
		});

		test('backup path is under data dir', () => {
			process.env['LET_HOME'] = TEST_HOME;
			const { derived, resolved } = resolvePaths();
			expect(derived.backup).toBe(join(resolved.data, 'let.db.bak'));
		});

		test('jsonExport path is under data dir', () => {
			process.env['LET_HOME'] = TEST_HOME;
			const { derived, resolved } = resolvePaths();
			expect(derived.jsonExport).toBe(join(resolved.data, 'let.db.json'));
		});

		test('sourceDb() returns correct path', () => {
			process.env['LET_HOME'] = TEST_HOME;
			const { derived, resolved } = resolvePaths();
			expect(derived.sourceDb('postcodes')).toBe(join(resolved.sources, 'postcodes.db'));
			expect(derived.sourceDb('broadband')).toBe(join(resolved.sources, 'broadband.db'));
		});

		test('cacheDir() returns correct path', () => {
			process.env['LET_HOME'] = TEST_HOME;
			const { derived, resolved } = resolvePaths();
			expect(derived.cacheDir('12345')).toBe(join(resolved.cache, '12345'));
		});

		test('cacheEntry() returns correct path', () => {
			process.env['LET_HOME'] = TEST_HOME;
			const { derived, resolved } = resolvePaths();
			expect(derived.cacheEntry('12345')).toBe(join(resolved.cache, '12345', 'data.json'));
		});

		test('envFile path is under config dir', () => {
			process.env['LET_HOME'] = TEST_HOME;
			const { derived, resolved } = resolvePaths();
			expect(derived.envFile).toBe(join(resolved.config, '.env'));
		});

		test('templateFile path is under config dir', () => {
			process.env['LET_HOME'] = TEST_HOME;
			const { derived, resolved } = resolvePaths();
			expect(derived.templateFile).toBe(join(resolved.config, 'let.config.template.toml'));
		});
	});

	describe('CLI flag overrides', () => {
		test('overrides take highest priority', () => {
			const { resolved } = resolvePaths({
				dataDir: '/tmp/test-data',
				cacheDir: '/tmp/test-cache',
				configDir: '/tmp/test-config',
				sourcesDir: '/tmp/test-sources',
			});
			expect(resolved.data).toBe('/tmp/test-data');
			expect(resolved.cache).toBe('/tmp/test-cache');
			expect(resolved.config).toBe('/tmp/test-config');
			expect(resolved.sources).toBe('/tmp/test-sources');
		});

		test('relative overrides resolve against cwd', () => {
			const { resolved } = resolvePaths({ dataDir: 'my-data' });
			expect(resolved.data).toBe(join(process.cwd(), 'my-data'));
		});
	});

	describe('category env vars', () => {
		test('LET_DATA_DIR overrides default', () => {
			process.env['LET_DATA_DIR'] = '/opt/let/data';
			const { resolved } = resolvePaths();
			expect(resolved.data).toBe('/opt/let/data');
		});

		test('LET_CONFIG_DIR overrides default', () => {
			process.env['LET_CONFIG_DIR'] = '/opt/let/config';
			const { resolved } = resolvePaths();
			expect(resolved.config).toBe('/opt/let/config');
		});

		test('LET_CACHE_DIR overrides default', () => {
			process.env['LET_CACHE_DIR'] = '/opt/let/cache';
			const { resolved } = resolvePaths();
			expect(resolved.cache).toBe('/opt/let/cache');
		});

		test('LET_SOURCES_DIR overrides default', () => {
			process.env['LET_SOURCES_DIR'] = '/opt/let/sources';
			const { resolved } = resolvePaths();
			expect(resolved.sources).toBe('/opt/let/sources');
		});

		test('CLI flags override env vars', () => {
			process.env['LET_DATA_DIR'] = '/opt/let/data';
			const { resolved } = resolvePaths({ dataDir: '/cli/data' });
			expect(resolved.data).toBe('/cli/data');
		});
	});

	describe('caching', () => {
		test('paths() returns cached result after resolvePaths()', () => {
			const first = resolvePaths();
			const second = paths();
			expect(second).toBe(first);
		});

		test('resetPaths() clears the cache', () => {
			resolvePaths({ dataDir: '/tmp/first' });
			resetPaths();
			const { resolved } = resolvePaths({ dataDir: '/tmp/second' });
			expect(resolved.data).toBe('/tmp/second');
		});

		test('paths() auto-resolves if not yet called', () => {
			// resetPaths already called in afterEach, so cache is empty
			resetPaths();
			const result = paths();
			expect(result.resolved).toBeDefined();
			expect(result.derived).toBeDefined();
		});

		test('resolvePaths with overrides forces re-resolution', () => {
			resolvePaths();
			const { resolved } = resolvePaths({ dataDir: '/override' });
			expect(resolved.data).toBe('/override');
		});
	});
});
