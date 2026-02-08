/**
 * Build Census 2021 tenure (TS054) database
 *
 * Usage: bun run scripts/sources/census.ts
 */

/* biome-ignore-all lint/suspicious/noConsole: Build script uses console for progress */

import { Database } from 'bun:sqlite';
import { createReadStream, mkdirSync } from 'node:fs';
import { join } from 'node:path';
import { createInterface } from 'node:readline';
import { Glob } from 'bun';
import { createBatchInserter, downloadFile, extractZip, findColumnIndex, parseCsvLine, SOURCES_DIR, toInt, withTempDir } from '../utils.ts';

/**
 * Source page: https://www.nomisweb.co.uk/sources/census_2021_bulk
 * Direct download: https://www.nomisweb.co.uk/output/census/2021/census2021-ts054.zip
 */
const TS054_URL = 'https://www.nomisweb.co.uk/output/census/2021/census2021-ts054.zip';
const DB_PATH = join(SOURCES_DIR, 'census.db');
mkdirSync(SOURCES_DIR, { recursive: true });

async function findLsoaCsv(extractDir: string): Promise<string> {
	const glob = new Glob('**/census2021-ts054-lsoa.csv');
	const files = Array.from(glob.scanSync(extractDir));
	if (files.length === 0) throw new Error('LSOA CSV not found in TS054 archive');
	return join(extractDir, files[0] ?? '');
}

interface ColumnIndices {
	geoCode: number;
	total: number;
	council: number;
	housing: number;
}

function parseHeader(line: string): ColumnIndices {
	const header = parseCsvLine(line);
	const geoCode = findColumnIndex(header, ['geography code', 'geography']);
	const total = findColumnIndex(header, ['all households', 'total households', 'households: total']);
	const council = findColumnIndex(header, ['social rented: council', 'rents from council', 'social rented: local authority', 'social rented']);
	const housing = findColumnIndex(header, ['social rented: housing association', 'other social rented', 'social rented: registered social landlord', 'housing association']);

	if (geoCode < 0 || total < 0 || council < 0 || housing < 0) {
		throw new Error('Required TS054 columns not found');
	}
	return { geoCode, total, council, housing };
}

function parseRow(line: string, idx: ColumnIndices): [string, number | null, number | null, number | null, number | null] | null {
	const cols = parseCsvLine(line);
	const lsoa = cols[idx.geoCode]?.trim();
	if (!lsoa) return null;

	const total = toInt(cols[idx.total]);
	const council = toInt(cols[idx.council]);
	const housing = toInt(cols[idx.housing]);
	const denom = total ?? 0;
	const socialPct = denom > 0 ? (((council ?? 0) + (housing ?? 0)) / denom) * 100 : null;

	return [lsoa, total, council, housing, socialPct ? Number.parseFloat(socialPct.toFixed(2)) : null];
}

async function buildDatabase(csvPath: string): Promise<void> {
	console.log('Building census tenure database...');
	console.log(`Source: ${csvPath}`);
	console.log(`Output: ${DB_PATH}\n`);

	const db = new Database(DB_PATH, { create: true });
	db.exec(`
		DROP TABLE IF EXISTS tenure;
		CREATE TABLE tenure (
			lsoa_code TEXT PRIMARY KEY,
			total_households INTEGER,
			council INTEGER,
			housing_association INTEGER,
			social_housing_pct REAL
		);
		CREATE INDEX idx_tenure_social_pct ON tenure(social_housing_pct);
	`);

	const inserter = createBatchInserter<[string, number | null, number | null, number | null, number | null]>(
		db,
		'INSERT INTO tenure (lsoa_code, total_households, council, housing_association, social_housing_pct) VALUES (?, ?, ?, ?, ?)',
		{ batchSize: 5000 },
	);

	const stream = createReadStream(csvPath, { encoding: 'utf-8' });
	const rl = createInterface({ input: stream, crlfDelay: Infinity });

	let idx: ColumnIndices | null = null;

	for await (const line of rl) {
		if (!idx) {
			idx = parseHeader(line);
			continue;
		}

		const row = parseRow(line, idx);
		if (row) inserter.add(row);
	}

	const totalRows = inserter.flush();
	console.log(`Inserted ${totalRows.toLocaleString()} LSOAs.`);
	db.close();
}

export async function build(): Promise<void> {
	await withTempDir('census', async (tempDir) => {
		const zipPath = join(tempDir, 'ts054.zip');
		console.log('Downloading Census TS054...');
		console.log(`URL: ${TS054_URL}\n`);
		await downloadFile(TS054_URL, zipPath);
		console.log('Download complete.\n');

		const extractDir = join(tempDir, 'extract');
		console.log('Extracting ZIP archive...');
		await extractZip(zipPath, extractDir);
		console.log('Extraction complete.\n');

		const csvPath = await findLsoaCsv(extractDir);
		await buildDatabase(csvPath);
	});
}

if (import.meta.main) {
	build().catch((err) => {
		console.error('Census build failed:', err);
		process.exit(1);
	});
}
