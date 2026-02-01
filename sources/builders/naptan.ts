/**
 * Build NaPTAN stops database
 *
 * Usage: bun run sources/builders/naptan.ts
 */

/* biome-ignore-all lint/suspicious/noConsole: Build script uses console for progress */

import { Database } from 'bun:sqlite';
import { createReadStream, mkdirSync } from 'node:fs';
import { join } from 'node:path';
import { createInterface } from 'node:readline';
import { createBatchInserter, downloadFile, findColumnIndex, parseCsvLine, progress, progressDone, withTempDir } from '../utils/index.ts';

/**
 * Source page: https://www.data.gov.uk/dataset/ff93ffc1-6656-47d8-9155-85ea0b8f2251/naptan
 * Direct download: https://naptan.api.dft.gov.uk/v1/access-nodes?dataFormat=csv
 */
const NAPTAN_CSV_URL = 'https://naptan.api.dft.gov.uk/v1/access-nodes?dataFormat=csv';
const DB_PATH = join(import.meta.dirname, '..', 'db', 'naptan.db');
mkdirSync(join(import.meta.dirname, '..', 'db'), { recursive: true });

type StopRow = [string, string | null, string | null, string | null, number | null, number | null];

interface ColumnIndices {
	atco: number;
	naptan: number;
	name: number;
	type: number;
	lat: number;
	lng: number;
}

function parseHeader(line: string): ColumnIndices {
	const header = parseCsvLine(line);
	const indices: ColumnIndices = {
		atco: findColumnIndex(header, ['atco', 'code']),
		naptan: findColumnIndex(header, ['naptan', 'code']),
		name: findColumnIndex(header, ['common', 'name']),
		type: findColumnIndex(header, ['stop type']),
		lat: findColumnIndex(header, ['latitude']),
		lng: findColumnIndex(header, ['longitude']),
	};
	if (indices.atco < 0 || indices.lat < 0 || indices.lng < 0) {
		throw new Error('Required NaPTAN columns not found');
	}
	return indices;
}

function optionalColumn(cols: string[], idx: number): string | null {
	if (idx < 0) return null;
	return cols[idx]?.trim() ?? null;
}

function parseCoordinate(raw: string | undefined): number | null {
	if (!raw) return null;
	const value = Number.parseFloat(raw);
	return Number.isNaN(value) ? null : value;
}

function parseRow(cols: string[], idx: ColumnIndices): StopRow | null {
	const atco = cols[idx.atco]?.trim();
	if (!atco) return null;
	return [atco, optionalColumn(cols, idx.naptan), optionalColumn(cols, idx.name), optionalColumn(cols, idx.type), parseCoordinate(cols[idx.lat]), parseCoordinate(cols[idx.lng])];
}

async function buildDatabase(csvPath: string): Promise<void> {
	console.log('Building NaPTAN database...');
	console.log(`Source: ${csvPath}`);
	console.log(`Output: ${DB_PATH}\n`);

	const db = new Database(DB_PATH, { create: true });
	db.exec(`
		DROP TABLE IF EXISTS stops;
		CREATE TABLE stops (
			atco_code TEXT PRIMARY KEY,
			naptan_code TEXT,
			common_name TEXT,
			stop_type TEXT,
			lat REAL,
			lng REAL
		);
		CREATE INDEX idx_stops_lat_lng ON stops(lat, lng);
	`);

	const inserter = createBatchInserter<StopRow>(db, 'INSERT INTO stops (atco_code, naptan_code, common_name, stop_type, lat, lng) VALUES (?, ?, ?, ?, ?, ?)', {
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
	console.log(`Inserted ${totalRows.toLocaleString()} stops.`);
	db.close();
}

export async function build(): Promise<void> {
	await withTempDir('naptan', async (tempDir) => {
		const csvPath = join(tempDir, 'naptan.csv');
		console.log('Downloading NaPTAN stops...');
		console.log(`URL: ${NAPTAN_CSV_URL}\n`);
		await downloadFile(NAPTAN_CSV_URL, csvPath);
		console.log('Download complete.\n');
		await buildDatabase(csvPath);
	});
}

if (import.meta.main) {
	build().catch((err) => {
		console.error('NaPTAN build failed:', err);
		process.exit(1);
	});
}
