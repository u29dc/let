/**
 * Tests for JSON envelope module.
 *
 * Uses subprocess spawn to verify stdout purity and exit codes,
 * since ok() and fail() call process.exit().
 */

import { describe, expect, test } from 'bun:test';
import { join } from 'node:path';

const HELPERS_DIR = join(import.meta.dirname, 'helpers');

function runHelper(script: string): { stdout: string; stderr: string; exitCode: number } {
	const result = Bun.spawnSync(['bun', 'run', join(HELPERS_DIR, script)], {
		env: { ...process.env, NODE_ENV: 'test' },
	});
	return {
		stdout: result.stdout.toString(),
		stderr: result.stderr.toString(),
		exitCode: result.exitCode,
	};
}

describe('envelope', () => {
	describe('ok()', () => {
		test('stdout is exactly one valid JSON object', () => {
			const { stdout, exitCode } = runHelper('envelope-ok.ts');
			const parsed = JSON.parse(stdout);
			expect(parsed.ok).toBe(true);
			expect(exitCode).toBe(0);
		});

		test('has required fields: ok, data, meta', () => {
			const { stdout } = runHelper('envelope-ok.ts');
			const parsed = JSON.parse(stdout);
			expect(parsed).toHaveProperty('ok', true);
			expect(parsed).toHaveProperty('data');
			expect(parsed).toHaveProperty('meta');
			expect(parsed.meta).toHaveProperty('tool', 'test.ok');
			expect(parsed.meta).toHaveProperty('elapsed');
			expect(typeof parsed.meta.elapsed).toBe('number');
		});

		test('no extra bytes before or after JSON', () => {
			const { stdout } = runHelper('envelope-ok.ts');
			const trimmed = stdout.trim();
			// Should parse as JSON with no leftover
			JSON.parse(trimmed);
			// No extra characters besides the JSON and trailing newline
			expect(stdout).toBe(`${trimmed}\n`);
		});

		test('includes optional meta fields when provided', () => {
			const { stdout } = runHelper('envelope-ok-meta.ts');
			const parsed = JSON.parse(stdout);
			expect(parsed.meta.count).toBe(5);
			expect(parsed.meta.total).toBe(100);
			expect(parsed.meta.hasMore).toBe(true);
		});
	});

	describe('fail()', () => {
		test('stdout is exactly one valid JSON error envelope', () => {
			const { stdout, exitCode } = runHelper('envelope-fail.ts');
			const parsed = JSON.parse(stdout);
			expect(parsed.ok).toBe(false);
			expect(exitCode).toBe(1);
		});

		test('has required error fields: code, message, hint', () => {
			const { stdout } = runHelper('envelope-fail.ts');
			const parsed = JSON.parse(stdout);
			expect(parsed.error).toHaveProperty('code', 'TEST_ERROR');
			expect(parsed.error).toHaveProperty('message', 'Something went wrong');
			expect(parsed.error).toHaveProperty('hint', 'Try again');
			expect(parsed.meta).toHaveProperty('tool', 'test.fail');
		});

		test('blocking codes exit with code 2', () => {
			const { exitCode } = runHelper('envelope-fail-blocking.ts');
			expect(exitCode).toBe(2);
		});

		test('non-blocking codes exit with code 1', () => {
			const { exitCode } = runHelper('envelope-fail.ts');
			expect(exitCode).toBe(1);
		});
	});

	describe('isJsonMode()', () => {
		test('detects --json flag', () => {
			const result = Bun.spawnSync(['bun', 'run', join(HELPERS_DIR, 'envelope-json-mode.ts'), '--json'], {
				env: { ...process.env, NODE_ENV: 'test' },
			});
			expect(result.stdout.toString().trim()).toBe('true');
		});

		test('returns false without --json flag', () => {
			const result = Bun.spawnSync(['bun', 'run', join(HELPERS_DIR, 'envelope-json-mode.ts')], {
				env: { ...process.env, NODE_ENV: 'test' },
			});
			expect(result.stdout.toString().trim()).toBe('false');
		});
	});
});
