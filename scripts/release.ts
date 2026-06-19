#!/usr/bin/env bun

import { spawn } from 'node:child_process';
import { chmod, copyFile, mkdir, mkdtemp, rename, rm } from 'node:fs/promises';
import { join } from 'node:path';
import { createInterface } from 'node:readline';

const REQUIRED_BINARIES = ['let', 'let-tui'] as const;
type RequiredBinary = (typeof REQUIRED_BINARIES)[number];

type InstallPlan = {
	name: RequiredBinary;
	source: string;
	destination: string;
	staged: string;
	backup: string;
	hadExisting: boolean;
};

const REQUIRED_BINARY_SET = new Set<string>(REQUIRED_BINARIES);

function installDir(): string {
	const letHome = process.env['LET_HOME'];
	const toolsHome = process.env['TOOLS_HOME'];
	const home = process.env['HOME'];

	if (letHome) {
		return letHome;
	}
	if (toolsHome) {
		return join(toolsHome, 'let');
	}
	if (!home) {
		throw new Error('HOME must be set when LET_HOME and TOOLS_HOME are unset');
	}
	return join(home, '.tools', 'let');
}

function outputName(name: RequiredBinary): string {
	return process.platform === 'win32' ? `${name}.exe` : name;
}

function writeRenderedDiagnostic(message: unknown): void {
	const rendered = (message as { message?: { rendered?: unknown } })?.message?.rendered;
	if (typeof rendered !== 'string' || rendered.length === 0) {
		return;
	}

	process.stderr.write(rendered.endsWith('\n') ? rendered : `${rendered}\n`);
}

async function runCargoBuild(): Promise<Map<RequiredBinary, string>> {
	const cargo = spawn(
		'cargo',
		['build', '--workspace', '--release', '--message-format=json-render-diagnostics'],
		{ stdio: ['ignore', 'pipe', 'inherit'] },
	);
	const artifacts = new Map<RequiredBinary, string>();
	const closePromise = new Promise<number>((resolve, reject) => {
		cargo.once('error', reject);
		cargo.once('close', (code) => resolve(code ?? 1));
	});
	const lines = createInterface({ input: cargo.stdout, crlfDelay: Infinity });

	for await (const line of lines) {
		if (line.trim().length === 0) {
			continue;
		}

		let message: unknown;
		try {
			message = JSON.parse(line);
		} catch {
			process.stderr.write(`${line}\n`);
			continue;
		}

		writeRenderedDiagnostic(message);
		const artifact = parseCargoArtifact(message);
		if (artifact) {
			artifacts.set(artifact.name, artifact.path);
		}
	}

	const exitCode = await closePromise;
	if (exitCode !== 0) {
		throw new Error(`cargo build failed with status ${exitCode}`);
	}

	return artifacts;
}

function parseCargoArtifact(message: unknown): { name: RequiredBinary; path: string } | null {
	const item = message as {
		reason?: unknown;
		target?: { kind?: unknown; name?: unknown };
		executable?: unknown;
	};
	const kind = item.target?.kind;
	const name = item.target?.name;
	if (
		item.reason !== 'compiler-artifact' ||
		!Array.isArray(kind) ||
		!kind.includes('bin') ||
		typeof name !== 'string' ||
		!REQUIRED_BINARY_SET.has(name) ||
		typeof item.executable !== 'string'
	) {
		return null;
	}
	return { name: name as RequiredBinary, path: item.executable };
}

async function installArtifacts(artifacts: Map<RequiredBinary, string>, outDir: string): Promise<void> {
	const missing = REQUIRED_BINARIES.filter((name) => !artifacts.has(name));
	if (missing.length > 0) {
		throw new Error(`cargo build did not report executable artifact(s): ${missing.join(', ')}`);
	}

	await mkdir(outDir, { recursive: true });
	const stagingDir = await mkdtemp(join(outDir, '.install-'));
	const plans = REQUIRED_BINARIES.map<InstallPlan>((name) => {
		const source = artifacts.get(name);
		if (!source) {
			throw new Error(`missing release artifact: ${name}`);
		}
		const fileName = outputName(name);
		return {
			name,
			source,
			destination: join(outDir, fileName),
			staged: join(stagingDir, fileName),
			backup: join(stagingDir, `${fileName}.previous`),
			hadExisting: false,
		};
	});
	const installed: InstallPlan[] = [];

	try {
		for (const plan of plans) {
			await copyFile(plan.source, plan.staged);
			if (process.platform !== 'win32') {
				await chmod(plan.staged, 0o755);
			}
		}

		for (const plan of plans) {
			try {
				await copyFile(plan.destination, plan.backup);
				plan.hadExisting = true;
			} catch (error) {
				if (!isNoEntryError(error)) {
					throw error;
				}
			}
		}

		for (const plan of plans) {
			await rename(plan.staged, plan.destination);
			installed.push(plan);
			process.stderr.write(`installed ${plan.name} -> ${plan.destination}\n`);
		}
	} catch (error) {
		await rollbackInstalled(installed);
		throw error;
	} finally {
		await rm(stagingDir, { recursive: true, force: true }).catch((error: unknown) => {
			process.stderr.write(
				`warning: failed to remove temporary install directory ${stagingDir}: ${error instanceof Error ? error.message : String(error)}\n`,
			);
		});
	}
}

function isNoEntryError(error: unknown): boolean {
	return typeof error === 'object' && error !== null && 'code' in error && error.code === 'ENOENT';
}

async function rollbackInstalled(installed: InstallPlan[]): Promise<void> {
	for (const plan of [...installed].reverse()) {
		if (plan.hadExisting) {
			await copyFile(plan.backup, plan.destination).catch(() => {});
		} else {
			await rm(plan.destination, { force: true }).catch(() => {});
		}
	}
}

async function main(): Promise<void> {
	const artifacts = await runCargoBuild();
	await installArtifacts(artifacts, installDir());
}

if (import.meta.main) {
	main().catch((error) => {
		process.stderr.write(`release: ${error instanceof Error ? error.message : String(error)}\n`);
		process.exit(1);
	});
}
