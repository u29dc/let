/**
 * Tests for health command.
 *
 * Uses subprocess spawn to test actual CLI output.
 * Minimizes spawns for speed (each bun startup takes ~1s).
 */

import { afterEach, describe, expect, test } from 'bun:test';
import { mkdirSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const CLI_ENTRY = join(import.meta.dirname, '..', 'src', 'index.ts');

function runHealth(env?: Record<string, string>): { stdout: string; exitCode: number } {
	const result = Bun.spawnSync(['bun', 'run', CLI_ENTRY, 'health', '--json'], {
		env: { ...process.env, ...env },
	});
	return {
		stdout: result.stdout.toString().trim(),
		exitCode: result.exitCode,
	};
}

function makeTempDir(): string {
	const dir = join(tmpdir(), `let-health-${Date.now()}-${Math.random().toString(36).slice(2)}`);
	mkdirSync(dir, { recursive: true });
	return dir;
}

const tempDirs: string[] = [];
afterEach(() => {
	for (const d of tempDirs) {
		try {
			rmSync(d, { recursive: true, force: true });
		} catch {
			// ignore
		}
	}
	tempDirs.length = 0;
});

describe('health command', () => {
	test('returns valid JSON with required structure (dev environment)', () => {
		const { stdout, exitCode } = runHealth();
		const parsed = JSON.parse(stdout);

		// Envelope structure
		expect(parsed.ok).toBe(true);
		expect(parsed.meta.tool).toBe('health');
		expect(typeof parsed.meta.elapsed).toBe('number');

		// Data structure
		const data = parsed.data;
		expect(data.status).toMatch(/^(ready|degraded|blocked)$/);
		expect(data.paths).toHaveProperty('config');
		expect(data.paths).toHaveProperty('data');
		expect(data.paths).toHaveProperty('cache');
		expect(data.paths).toHaveProperty('sources');
		expect(typeof data.paths.isDev).toBe('boolean');

		// Checks
		expect(Array.isArray(data.checks)).toBe(true);
		expect(data.checks.length).toBeGreaterThan(0);

		// Summary
		expect(typeof data.summary.ok).toBe('number');
		expect(typeof data.summary.blocking).toBe('number');
		expect(typeof data.summary.degraded).toBe('number');

		// Exit code 2 for blocked (missing config in dev env)
		if (data.status === 'blocked') {
			expect(exitCode).toBe(2);
		}
	});

	test('reports missing sources with correct severity', () => {
		const tempDir = makeTempDir();
		tempDirs.push(tempDir);
		const { stdout } = runHealth({
			LET_SOURCES_DIR: join(tempDir, 'empty'),
		});
		const { data } = JSON.parse(stdout);
		const sourceChecks = data.checks.filter((c: Record<string, string>) => (c['id'] ?? '').startsWith('source.'));
		expect(sourceChecks.length).toBe(10);

		// postcodes is blocking, rest are degraded
		const postcodes = sourceChecks.find((c: Record<string, string>) => c['id'] === 'source.postcodes');
		expect(postcodes.severity).toBe('blocking');
		expect(postcodes.status).toBe('missing');

		const broadband = sourceChecks.find((c: Record<string, string>) => c['id'] === 'source.broadband');
		expect(broadband.severity).toBe('degraded');
	});

	test('fix commands use resolved paths', () => {
		const tempDir = makeTempDir();
		tempDirs.push(tempDir);
		const { stdout } = runHealth({
			LET_CONFIG_DIR: tempDir,
			LET_DATA_DIR: tempDir,
			LET_CACHE_DIR: join(tempDir, 'cache'),
			LET_SOURCES_DIR: join(tempDir, 'sources'),
		});
		const { data } = JSON.parse(stdout);
		const configCheck = data.checks.find((c: Record<string, string>) => c['id'] === 'config');
		expect(configCheck.fix).toBeDefined();
		// Fix commands should reference the temp dir, not hardcoded paths
		for (const fix of configCheck.fix) {
			expect(fix).toContain(tempDir);
		}
	});

	test('config check is ok when config file exists', () => {
		const tempDir = makeTempDir();
		tempDirs.push(tempDir);
		// In dev mode, config file is let.config.toml
		writeFileSync(join(tempDir, 'let.config.toml'), '[search]\n');
		const { stdout } = runHealth({ LET_CONFIG_DIR: tempDir });
		const { data } = JSON.parse(stdout);
		const configCheck = data.checks.find((c: Record<string, string>) => c['id'] === 'config');
		expect(configCheck.status).toBe('ok');
	});
});
