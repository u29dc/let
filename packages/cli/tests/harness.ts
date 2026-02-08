/**
 * In-process test harness for CLI commands.
 *
 * Runs commands via citty's runCommand() with capture mode enabled,
 * avoiding subprocess overhead (~48ms per test -> ~1ms per test).
 *
 * The harness:
 * 1. Enables capture mode so ok()/fail()/emitRaw() throw EnvelopeCapture
 * 2. Overrides isJsonMode() without mutating process.argv (ref-counted)
 * 3. Resets path cache between runs so env var overrides take effect
 * 4. Catches EnvelopeCapture and returns { stdout, exitCode }
 */

import { resetPaths, resolvePaths } from '@let/core/paths';
import { runCommand } from 'citty';
import { EnvelopeCapture, setCaptureMode, setJsonModeOverride } from '../src/envelope.js';
import { main } from '../src/main.js';

export interface RunResult {
	stdout: string;
	exitCode: number;
}

/**
 * Run a CLI command in-process with capture mode.
 *
 * @param rawArgs - Command args (e.g. ['view', 'list', '--json'])
 * @param env - Environment variable overrides (applied to process.env temporarily)
 */
export async function run(rawArgs: string[], env?: Record<string, string>): Promise<RunResult> {
	const savedEnv: Record<string, string | undefined> = {};
	const hasJson = rawArgs.includes('--json');

	try {
		// Apply env overrides
		if (env) {
			for (const [key, value] of Object.entries(env)) {
				savedEnv[key] = process.env[key];
				process.env[key] = value;
			}
		}

		// Reset path cache so env vars take effect
		resetPaths();
		resolvePaths();

		// Enable capture mode + json mode override (ref-counted for concurrent safety)
		setCaptureMode(true);
		if (hasJson) setJsonModeOverride(true);

		await runCommand(main, { rawArgs });

		// If we get here, command completed without calling ok()/fail()
		return { stdout: '', exitCode: 0 };
	} catch (e) {
		if (e instanceof EnvelopeCapture) {
			return { stdout: e.envelope, exitCode: e.exitCode };
		}
		throw e;
	} finally {
		setCaptureMode(false);
		if (hasJson) setJsonModeOverride(false);

		// Restore env
		for (const [key, value] of Object.entries(savedEnv)) {
			if (value === undefined) {
				delete process.env[key];
			} else {
				process.env[key] = value;
			}
		}

		// Reset paths for next run
		resetPaths();
	}
}
