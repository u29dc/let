/**
 * Shared utilities for build scripts
 *
 * Merged from: sources/utils/index.ts, build-skill.ts, build-all.ts
 * Used by: scripts/build-skill.ts, scripts/build-sources.ts, scripts/sources/*.ts
 */

import type { Database } from 'bun:sqlite';
import { createWriteStream } from 'node:fs';
import { mkdir, mkdtemp, readdir } from 'node:fs/promises';
import { homedir, tmpdir } from 'node:os';
import { join, resolve } from 'node:path';

// ============================================================================
// Paths
// ============================================================================

export const ROOT = resolve(import.meta.dirname, '..');

function resolveSourcesDir(): string {
	const letHome = process.env['LET_HOME'] || join(process.env['TOOLS_HOME'] || join(homedir(), '.tools'), 'let');
	return join(letHome, 'sources');
}

export const SOURCES_DIR = resolveSourcesDir();

// ============================================================================
// Formatting
// ============================================================================

export function formatSize(bytes: number): string {
	if (bytes >= 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GB`;
	if (bytes >= 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
	if (bytes >= 1024) return `${(bytes / 1024).toFixed(1)} KB`;
	return `${bytes} B`;
}

export function formatElapsed(ms: number): string {
	return `${(ms / 1000).toFixed(1)}s`;
}

// ============================================================================
// Progress output (TTY-aware)
// ============================================================================

export const isTTY = process.stdout.isTTY ?? false;
const isParallel = Boolean(process.env['BUILD_PARALLEL']);

export function progress(msg: string, pct?: number): void {
	if (isParallel) {
		process.stdout.write(`\x01P:${JSON.stringify({ msg, pct: pct ?? null })}\n`);
		return;
	}
	if (isTTY) process.stdout.write(`\r${msg}`);
}

export function progressDone(): void {
	if (isParallel) return;
	if (isTTY) process.stdout.write('\n');
}

// ============================================================================
// SAS token expiry check
// ============================================================================

export interface SasExpiryContext {
	sourceName: string;
	sourcePageUrl: string;
	envUrlVar: string;
	envPathVar: string;
	buildCommand: string;
}

export function checkSasExpiry(url: string, ctx: SasExpiryContext): void {
	const parsed = new URL(url);
	const seParam = parsed.searchParams.get('se');
	if (!seParam) return;

	const expiry = new Date(seParam);
	if (Number.isNaN(expiry.getTime())) return;

	const now = new Date();
	const msUntilExpiry = expiry.getTime() - now.getTime();

	if (msUntilExpiry <= 0) {
		const expiryStr = expiry.toISOString().slice(0, 10);
		throw new Error(
			`SAS token expired for ${ctx.sourceName} dataset (expired ${expiryStr})\n\n` +
				`Get a fresh URL from: ${ctx.sourcePageUrl}\n` +
				`Then either:\n` +
				`  ${ctx.envUrlVar}="<new-url>" ${ctx.buildCommand}\n` +
				`  ${ctx.envPathVar}="/path/to/local.csv" ${ctx.buildCommand}`,
		);
	}

	const hoursUntilExpiry = msUntilExpiry / (1000 * 60 * 60);
	if (hoursUntilExpiry <= 24) {
		// biome-ignore lint/suspicious/noConsole: build script warning to stderr
		console.warn(`WARNING: SAS token for ${ctx.sourceName} expires in ${Math.round(hoursUntilExpiry)}h (${expiry.toISOString().slice(0, 10)})`);
	}
}

// ============================================================================
// Download with streaming and progress
// ============================================================================

export interface DownloadOptions {
	headers?: Record<string, string>;
}

export async function downloadFile(url: string, dest: string, options?: DownloadOptions): Promise<void> {
	const response = await fetch(url, { headers: options?.headers });
	if (!response.ok) {
		throw new Error(`Download failed: ${response.status} ${response.statusText}`);
	}
	if (!response.body) {
		throw new Error('Response body is null');
	}

	const contentLength = response.headers.get('content-length');
	const totalBytes = contentLength ? Number.parseInt(contentLength, 10) : 0;

	const fileStream = createWriteStream(dest);
	const reader = response.body.getReader();
	let downloadedBytes = 0;

	try {
		while (true) {
			const { done, value } = await reader.read();
			if (done) break;

			fileStream.write(value);
			downloadedBytes += value.length;

			const mb = (downloadedBytes / 1024 / 1024).toFixed(1);
			if (totalBytes > 0) {
				const pctNum = (downloadedBytes / totalBytes) * 100;
				const totalMb = (totalBytes / 1024 / 1024).toFixed(1);
				progress(`Downloading: ${mb}MB / ${totalMb}MB (${pctNum.toFixed(1)}%)`, Math.round(pctNum));
			} else {
				progress(`Downloading: ${mb}MB`);
			}
		}
	} finally {
		fileStream.end();
	}
	progressDone();
}

// ============================================================================
// ZIP extraction
// ============================================================================

export async function extractZip(zipPath: string, destDir: string): Promise<void> {
	await mkdir(destDir, { recursive: true });
	const result = Bun.spawnSync(['unzip', '-q', zipPath, '-d', destDir]);
	if (result.exitCode !== 0) {
		throw new Error(`unzip failed: ${result.stderr.toString()}`);
	}
}

export async function findNestedZip(dir: string, pattern: string, maxDepth = 3): Promise<string | null> {
	async function search(currentDir: string, depth: number): Promise<string | null> {
		if (depth > maxDepth) return null;
		const entries = await readdir(currentDir, { withFileTypes: true });
		for (const entry of entries) {
			if (entry.isFile() && entry.name.includes(pattern) && entry.name.endsWith('.zip')) {
				return join(currentDir, entry.name);
			}
			if (entry.isDirectory()) {
				const found = await search(join(currentDir, entry.name), depth + 1);
				if (found) return found;
			}
		}
		return null;
	}
	return search(dir, 0);
}

// ============================================================================
// CSV parsing (RFC-4180 compliant)
// ============================================================================

export function parseCsvLine(line: string): string[] {
	const result: string[] = [];
	let current = '';
	let inQuotes = false;

	for (let i = 0; i < line.length; i++) {
		const char = line[i];
		if (char === '"') {
			const next = line[i + 1];
			if (inQuotes && next === '"') {
				current += '"';
				i += 1;
				continue;
			}
			inQuotes = !inQuotes;
			continue;
		}
		if (char === ',' && !inQuotes) {
			result.push(current);
			current = '';
			continue;
		}
		current += char ?? '';
	}

	result.push(current);
	return result;
}

export function normalizePostcode(value: string): string {
	return value.replace(/\s+/g, '').toUpperCase();
}

export function cleanHeader(value: string): string {
	return value.trim().replace(/^"|"$/g, '').toLowerCase();
}

export function toNumber(value: string | undefined): number | null {
	if (!value) return null;
	const parsed = Number.parseFloat(value);
	return Number.isNaN(parsed) ? null : parsed;
}

export function toInt(value: string | undefined): number | null {
	if (!value) return null;
	const parsed = Number.parseInt(value, 10);
	return Number.isNaN(parsed) ? null : parsed;
}

export function findColumnIndex(headers: string[], patterns: string[]): number {
	const lowered = headers.map((h) => h.toLowerCase());
	for (const pattern of patterns) {
		const idx = lowered.findIndex((h) => h.includes(pattern));
		if (idx >= 0) return idx;
	}
	return -1;
}

export function findColumnIndices<T extends Record<string, string[]>>(headers: string[], columns: T): Record<keyof T, number> {
	const result = {} as Record<keyof T, number>;
	for (const [key, patterns] of Object.entries(columns)) {
		result[key as keyof T] = findColumnIndex(headers, patterns);
	}
	return result;
}

// ============================================================================
// Database batch insert
// ============================================================================

export interface BatchInserterOptions {
	batchSize?: number;
	onProgress?: (count: number) => void;
}

export interface BatchInserter<T extends unknown[]> {
	add: (params: T) => void;
	flush: () => number;
}

export function createBatchInserter<T extends unknown[]>(db: Database, sql: string, options?: BatchInserterOptions): BatchInserter<T> {
	const batchSize = options?.batchSize ?? 10000;
	const onProgress = options?.onProgress;
	const stmt = db.prepare(sql);
	const batch: T[] = [];
	let totalInserted = 0;

	const insertMany = db.transaction((rows: T[]) => {
		for (const row of rows) stmt.run(...row);
	});

	function flush(): number {
		if (batch.length > 0) {
			insertMany(batch);
			totalInserted += batch.length;
			batch.length = 0;
			onProgress?.(totalInserted);
		}
		return totalInserted;
	}

	function add(params: T): void {
		batch.push(params);
		if (batch.length >= batchSize) {
			flush();
		}
	}

	return { add, flush };
}

// ============================================================================
// Temp directory management
// ============================================================================

export async function withTempDir<T>(prefix: string, fn: (dir: string) => Promise<T>): Promise<T> {
	const tempDir = await mkdtemp(join(tmpdir(), `let-${prefix}-`));
	try {
		return await fn(tempDir);
	} finally {
		Bun.spawnSync(['chmod', '-R', 'u+w', tempDir]);
		Bun.spawnSync(['rm', '-rf', tempDir]);
	}
}
