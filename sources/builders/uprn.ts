/**
 * Build OS Open UPRN lookup database
 *
 * Usage: bun run sources/builders/uprn.ts
 */

/* biome-ignore-all lint/suspicious/noConsole: Build script uses console for progress */

import { Database } from 'bun:sqlite';
import { createReadStream, mkdirSync } from 'node:fs';
import { join } from 'node:path';
import { createInterface } from 'node:readline';
import { Glob } from 'bun';
import { createBatchInserter, downloadFile, extractZip, findColumnIndex, parseCsvLine, progress, progressDone, toNumber, withTempDir } from '../utils/index.ts';

/**
 * Source page: https://osdatahub.os.uk/data/downloads/open/OpenUPRN
 * Direct download: https://api.os.uk/downloads/v1/products/OpenUPRN/downloads?area=GB&format=CSV&redirect
 * Notes:
 * - Product overview: https://www.ordnancesurvey.co.uk/products/os-open-uprn
 */
const UPRN_ZIP_URL = 'https://api.os.uk/downloads/v1/products/OpenUPRN/downloads?area=GB&format=CSV&redirect';
const DB_PATH = join(import.meta.dirname, '..', 'db', 'uprn.db');
mkdirSync(join(import.meta.dirname, '..', 'db'), { recursive: true });

async function findCsvFile(extractDir: string): Promise<string> {
	const glob = new Glob('**/*.csv');
	const files = Array.from(glob.scanSync(extractDir));
	if (files.length === 0) throw new Error('UPRN CSV not found in archive');
	return join(extractDir, files[0] ?? '');
}

interface ColumnIndices {
	uprn: number;
	lat: number;
	lng: number;
	x: number;
	y: number;
}

function parseHeader(line: string): ColumnIndices {
	const header = parseCsvLine(line);
	const uprnIdx = findColumnIndex(header, ['uprn']);
	if (uprnIdx < 0) throw new Error('UPRN column not found');
	return {
		uprn: uprnIdx,
		lat: findColumnIndex(header, ['lat']),
		lng: findColumnIndex(header, ['long', 'lng']),
		x: findColumnIndex(header, ['x']),
		y: findColumnIndex(header, ['y']),
	};
}

function parseRow(cols: string[], idx: ColumnIndices): [string, number | null, number | null, number | null, number | null] | null {
	const uprn = cols[idx.uprn]?.trim();
	if (!uprn) return null;
	return [uprn, idx.lat >= 0 ? toNumber(cols[idx.lat]) : null, idx.lng >= 0 ? toNumber(cols[idx.lng]) : null, idx.x >= 0 ? toNumber(cols[idx.x]) : null, idx.y >= 0 ? toNumber(cols[idx.y]) : null];
}

async function buildDatabase(csvPath: string): Promise<void> {
	console.log('Building UPRN database...');
	console.log(`Source: ${csvPath}`);
	console.log(`Output: ${DB_PATH}\n`);

	const db = new Database(DB_PATH, { create: true });
	db.exec(`
		DROP TABLE IF EXISTS uprn;
		CREATE TABLE uprn (
			uprn TEXT PRIMARY KEY,
			lat REAL,
			lng REAL,
			x REAL,
			y REAL
		);
		CREATE INDEX idx_uprn_lat_lng ON uprn(lat, lng);
	`);

	const inserter = createBatchInserter<[string, number | null, number | null, number | null, number | null]>(db, 'INSERT INTO uprn (uprn, lat, lng, x, y) VALUES (?, ?, ?, ?, ?)', {
		batchSize: 10000,
		onProgress: (n) => progress(`Processed ${n.toLocaleString()} rows`),
	});

	const stream = createReadStream(csvPath, { encoding: 'utf-8' });
	const rl = createInterface({ input: stream, crlfDelay: Infinity });

	let idx: ColumnIndices | null = null;

	for await (const line of rl) {
		if (!idx) {
			idx = parseHeader(line);
			continue;
		}

		const row = parseRow(parseCsvLine(line), idx);
		if (row) inserter.add(row);
	}

	const totalRows = inserter.flush();
	progressDone();
	console.log(`Inserted ${totalRows.toLocaleString()} UPRNs.`);
	db.close();
}

export async function build(): Promise<void> {
	await withTempDir('uprn', async (tempDir) => {
		const zipPath = join(tempDir, 'uprn.zip');
		console.log('Downloading OS Open UPRN...');
		console.log(`URL: ${UPRN_ZIP_URL}\n`);
		await downloadFile(UPRN_ZIP_URL, zipPath);
		console.log('Download complete.\n');

		const extractDir = join(tempDir, 'extract');
		console.log('Extracting ZIP archive...');
		await extractZip(zipPath, extractDir);
		Bun.spawnSync(['chmod', '-R', 'u+rw', extractDir]);
		console.log('Extraction complete.\n');

		const csvPath = await findCsvFile(extractDir);
		await buildDatabase(csvPath);
	});
}

if (import.meta.main) {
	build().catch((err) => {
		console.error('UPRN build failed:', err);
		process.exit(1);
	});
}
