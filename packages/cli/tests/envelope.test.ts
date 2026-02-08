/**
 * Tests for JSON envelope module.
 *
 * ok() and fail() tests use capture mode for in-process testing.
 * isJsonMode() tests remain as subprocess (they test process.argv detection).
 */

import { describe, expect, test } from 'bun:test';
import { join } from 'node:path';
import { EnvelopeCapture, fail, ok, setCaptureMode } from '../src/envelope.js';

const HELPERS_DIR = join(import.meta.dirname, 'helpers');

/** Call ok()/fail() in capture mode and return the captured result. */
function capture(fn: () => void): { envelope: string; exitCode: number } {
	setCaptureMode(true);
	try {
		fn();
		throw new Error('Expected EnvelopeCapture to be thrown');
	} catch (e) {
		if (e instanceof EnvelopeCapture) {
			return { envelope: e.envelope, exitCode: e.exitCode };
		}
		throw e;
	} finally {
		setCaptureMode(false);
	}
}

describe('envelope', () => {
	describe('ok()', () => {
		test('returns valid JSON object', () => {
			const { envelope, exitCode } = capture(() => ok('test.ok', { items: [1, 2, 3] }, performance.now()));
			const parsed = JSON.parse(envelope);
			expect(parsed.ok).toBe(true);
			expect(exitCode).toBe(0);
		});

		test('has required fields: ok, data, meta', () => {
			const { envelope } = capture(() => ok('test.ok', { items: [1, 2, 3] }, performance.now()));
			const parsed = JSON.parse(envelope);
			expect(parsed).toHaveProperty('ok', true);
			expect(parsed).toHaveProperty('data');
			expect(parsed).toHaveProperty('meta');
			expect(parsed.meta).toHaveProperty('tool', 'test.ok');
			expect(parsed.meta).toHaveProperty('elapsed');
			expect(typeof parsed.meta.elapsed).toBe('number');
		});

		test('no extra bytes in JSON', () => {
			const { envelope } = capture(() => ok('test.ok', { items: [1, 2, 3] }, performance.now()));
			// Should parse cleanly
			JSON.parse(envelope);
			// No newlines or whitespace in the JSON string itself
			expect(envelope).toBe(envelope.trim());
		});

		test('includes optional meta fields when provided', () => {
			const { envelope } = capture(() => ok('test.ok', { items: [1, 2, 3, 4, 5] }, performance.now(), { count: 5, total: 100, hasMore: true }));
			const parsed = JSON.parse(envelope);
			expect(parsed.meta.count).toBe(5);
			expect(parsed.meta.total).toBe(100);
			expect(parsed.meta.hasMore).toBe(true);
		});
	});

	describe('fail()', () => {
		test('returns valid JSON error envelope', () => {
			const { envelope, exitCode } = capture(() => fail('test.fail', 'TEST_ERROR', 'Something went wrong', 'Try again', performance.now()));
			const parsed = JSON.parse(envelope);
			expect(parsed.ok).toBe(false);
			expect(exitCode).toBe(1);
		});

		test('has required error fields: code, message, hint', () => {
			const { envelope } = capture(() => fail('test.fail', 'TEST_ERROR', 'Something went wrong', 'Try again', performance.now()));
			const parsed = JSON.parse(envelope);
			expect(parsed.error).toHaveProperty('code', 'TEST_ERROR');
			expect(parsed.error).toHaveProperty('message', 'Something went wrong');
			expect(parsed.error).toHaveProperty('hint', 'Try again');
			expect(parsed.meta).toHaveProperty('tool', 'test.fail');
		});

		test('blocking codes exit with code 2', () => {
			const { exitCode } = capture(() => fail('test.fail', 'NO_CONFIG', 'Config not found', 'Copy template', performance.now()));
			expect(exitCode).toBe(2);
		});

		test('non-blocking codes exit with code 1', () => {
			const { exitCode } = capture(() => fail('test.fail', 'TEST_ERROR', 'Something went wrong', 'Try again', performance.now()));
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
