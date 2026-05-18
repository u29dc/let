import { expect, test } from 'bun:test';

import { releaseArtifactFromCargoMessage, resolveInstallDir } from './build-release.mjs';

test('resolves the install directory with the same precedence as package build', () => {
	expect(resolveInstallDir({ LET_HOME: '/tmp/let-home', TOOLS_HOME: '/tmp/tools', HOME: '/tmp/home' })).toBe('/tmp/let-home');
	expect(resolveInstallDir({ TOOLS_HOME: '/tmp/tools', HOME: '/tmp/home' })).toBe('/tmp/tools/let');
	expect(resolveInstallDir({ HOME: '/tmp/home' })).toBe('/tmp/home/.tools/let');
});

test('uses Cargo-reported executable paths for required release binaries', () => {
	expect(
		releaseArtifactFromCargoMessage({
			reason: 'compiler-artifact',
			target: { kind: ['bin'], name: 'let' },
			executable: '/tmp/custom-target/release/let',
		}),
	).toEqual({ name: 'let', executable: '/tmp/custom-target/release/let' });

	expect(
		releaseArtifactFromCargoMessage({
			reason: 'compiler-artifact',
			target: { kind: ['bin'], name: 'let-tui' },
			executable: '/tmp/custom-target/release/let-tui',
		}),
	).toEqual({ name: 'let-tui', executable: '/tmp/custom-target/release/let-tui' });
});

test('ignores non-binary and unrelated Cargo artifacts', () => {
	expect(
		releaseArtifactFromCargoMessage({
			reason: 'compiler-artifact',
			target: { kind: ['lib'], name: 'let-sdk' },
			executable: null,
		}),
	).toBeNull();
	expect(
		releaseArtifactFromCargoMessage({
			reason: 'compiler-artifact',
			target: { kind: ['bin'], name: 'helper' },
			executable: '/tmp/custom-target/release/helper',
		}),
	).toBeNull();
});
