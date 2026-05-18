#!/usr/bin/env bun

import { spawn } from 'node:child_process';
import { chmod, copyFile, mkdir } from 'node:fs/promises';
import { join } from 'node:path';
import { createInterface } from 'node:readline';
import { pathToFileURL } from 'node:url';

const REQUIRED_BINARIES = ['let', 'let-tui'];
const REQUIRED_BINARY_SET = new Set(REQUIRED_BINARIES);

export function resolveInstallDir(env = process.env) {
	if (env.LET_HOME) {
		return env.LET_HOME;
	}

	if (env.TOOLS_HOME) {
		return join(env.TOOLS_HOME, 'let');
	}

	if (!env.HOME) {
		throw new Error('HOME must be set when LET_HOME and TOOLS_HOME are unset');
	}

	return join(env.HOME, '.tools', 'let');
}

export function releaseArtifactFromCargoMessage(message) {
	if (!message || message.reason !== 'compiler-artifact') {
		return null;
	}

	const target = message.target;
	if (!target || !Array.isArray(target.kind) || !target.kind.includes('bin')) {
		return null;
	}

	const name = typeof target.name === 'string' ? target.name : '';
	if (!REQUIRED_BINARY_SET.has(name) || typeof message.executable !== 'string') {
		return null;
	}

	return { name, executable: message.executable };
}

function writeRenderedDiagnostic(message) {
	const rendered = message?.message?.rendered;
	if (typeof rendered !== 'string' || rendered.length === 0) {
		return;
	}

	process.stderr.write(rendered.endsWith('\n') ? rendered : `${rendered}\n`);
}

async function runCargoBuild() {
	const cargo = spawn('cargo', ['build', '--workspace', '--release', '--message-format=json-render-diagnostics'], { stdio: ['ignore', 'pipe', 'inherit'] });
	const artifacts = new Map();
	const closePromise = new Promise((resolve, reject) => {
		cargo.once('error', reject);
		cargo.once('close', (code) => resolve(code ?? 1));
	});
	const lines = createInterface({ input: cargo.stdout, crlfDelay: Infinity });

	for await (const line of lines) {
		if (line.trim().length === 0) {
			continue;
		}

		let message;
		try {
			message = JSON.parse(line);
		} catch {
			process.stderr.write(`${line}\n`);
			continue;
		}

		writeRenderedDiagnostic(message);
		const artifact = releaseArtifactFromCargoMessage(message);
		if (artifact) {
			artifacts.set(artifact.name, artifact.executable);
		}
	}

	const exitCode = await closePromise;
	if (exitCode !== 0) {
		throw new Error(`cargo build failed with status ${exitCode}`);
	}

	return artifacts;
}

async function installArtifacts(artifacts, outDir) {
	const missing = REQUIRED_BINARIES.filter((name) => !artifacts.has(name));
	if (missing.length > 0) {
		throw new Error(`cargo build did not report executable artifact(s): ${missing.join(', ')}`);
	}

	await mkdir(outDir, { recursive: true });
	for (const name of REQUIRED_BINARIES) {
		const source = artifacts.get(name);
		const destination = join(outDir, name);
		await copyFile(source, destination);
		if (process.platform !== 'win32') {
			await chmod(destination, 0o755);
		}
		process.stderr.write(`installed ${name} -> ${destination}\n`);
	}
}

async function main() {
	const artifacts = await runCargoBuild();
	await installArtifacts(artifacts, resolveInstallDir());
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? '').href) {
	main().catch((error) => {
		process.stderr.write(`build-release: ${error.message}\n`);
		process.exit(1);
	});
}
