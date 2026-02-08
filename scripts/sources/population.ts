/**
 * Build population database from Census 2021 TS001
 *
 * Usage: bun run scripts/sources/population.ts
 */

/* biome-ignore-all lint/suspicious/noConsole: Build script uses console for progress */

import { Database } from 'bun:sqlite';
import { mkdirSync } from 'node:fs';
import { join } from 'node:path';
import { Glob } from 'bun';
import { createBatchInserter, downloadFile, extractZip, findColumnIndex, parseCsvLine, SOURCES_DIR, toInt, withTempDir } from '../utils.ts';

/**
 * Source page: https://www.nomisweb.co.uk/sources/census_2021_bulk
 * Direct download: https://www.nomisweb.co.uk/output/census/2021/census2021-ts001.zip
 */
const TS001_ZIP_URL = 'https://www.nomisweb.co.uk/output/census/2021/census2021-ts001.zip';
const DB_PATH = join(SOURCES_DIR, 'population.db');
mkdirSync(SOURCES_DIR, { recursive: true });

async function findLsoaCsv(extractDir: string): Promise<string> {
	const glob = new Glob('**/census2021-ts001-lsoa.csv');
	const files = Array.from(glob.scanSync(extractDir));
	if (files.length === 0) throw new Error('LSOA CSV not found in TS001 archive');
	return join(extractDir, files[0] ?? '');
}

async function buildDatabase(csvPath: string): Promise<void> {
	console.log('Building population database...');
	console.log(`Source: ${csvPath}`);
	console.log(`Output: ${DB_PATH}\n`);

	const content = await Bun.file(csvPath).text();
	const lines = content.split(/\r?\n/).filter(Boolean);
	if (lines.length === 0) throw new Error('Empty TS001 CSV');

	const header = parseCsvLine(lines[0] ?? '');
	const geoCodeIdx = findColumnIndex(header, ['geography code']);
	const totalIdx = findColumnIndex(header, ['all usual residents', 'total']);

	if (geoCodeIdx < 0 || totalIdx < 0) {
		throw new Error('Required TS001 columns not found');
	}

	const db = new Database(DB_PATH, { create: true });
	db.exec(`
		DROP TABLE IF EXISTS population;
		CREATE TABLE population (
			lsoa_code TEXT PRIMARY KEY,
			population INTEGER
		);
	`);

	const inserter = createBatchInserter<[string, number | null]>(db, 'INSERT INTO population (lsoa_code, population) VALUES (?, ?)', { batchSize: 5000 });

	for (let i = 1; i < lines.length; i++) {
		const cols = parseCsvLine(lines[i] ?? '');
		const lsoa = cols[geoCodeIdx]?.trim();
		if (!lsoa) continue;
		inserter.add([lsoa, toInt(cols[totalIdx])]);
	}

	const totalRows = inserter.flush();
	console.log(`Inserted ${totalRows.toLocaleString()} LSOAs.`);
	db.close();
}

export async function build(): Promise<void> {
	await withTempDir('population', async (tempDir) => {
		const zipPath = join(tempDir, 'ts001.zip');
		console.log('Downloading Census TS001...');
		console.log(`URL: ${TS001_ZIP_URL}\n`);
		await downloadFile(TS001_ZIP_URL, zipPath);
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
		console.error('Population build failed:', err);
		process.exit(1);
	});
}
