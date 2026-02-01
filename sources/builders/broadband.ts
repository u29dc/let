/**
 * Build broadband coverage database from Ofcom CSV data
 *
 * Usage: bun run sources/builders/broadband.ts
 *
 * @file Standalone build script - console output and complexity are expected
 */

/* biome-ignore-all lint/suspicious/noConsole: Build script uses console for progress */
/* biome-ignore-all lint/complexity/noExcessiveCognitiveComplexity: Build script has inherent complexity */

import { Database } from 'bun:sqlite';
import { mkdirSync } from 'node:fs';
import { mkdir, readdir } from 'node:fs/promises';
import { join } from 'node:path';
import { Glob } from 'bun';
import { downloadFile, extractZip, findNestedZip, progress, progressDone, withTempDir } from '../utils/index.ts';

/**
 * Source page: https://www.ofcom.org.uk/siteassets/resources/documents/research-and-data/multi-sector/infrastructure-research/connected-nations-2025/
 * Direct download: https://www.ofcom.org.uk/siteassets/resources/documents/research-and-data/multi-sector/infrastructure-research/connected-nations-2025/202507_fixed_broadband_coverage_r01.zip
 * Expected archive structure:
 * - Top-level ZIP contains a nested ZIP: 202507_fixed_coverage_r01/202507_fixed_pc_coverage_r01.zip
 * - Nested ZIP expands to postcode CSVs under: postcode_res_files/
 * Notes:
 * - Ofcom updates filenames by year/month; if the ZIP names or folders change, update the nested path detection.
 */
const OFCOM_ZIP_URL = 'https://www.ofcom.org.uk/siteassets/resources/documents/research-and-data/multi-sector/infrastructure-research/connected-nations-2025/202507_fixed_broadband_coverage_r01.zip';

const DB_PATH = join(import.meta.dirname, '..', 'db', 'broadband.db');
mkdirSync(join(import.meta.dirname, '..', 'db'), { recursive: true });

function extractOutward(postcodeWithSpace: string): string {
	return postcodeWithSpace.trim().split(' ')[0] ?? '';
}

function safeParseFloat(val: string): number {
	const n = Number.parseFloat(val);
	return Number.isNaN(n) ? 0 : n;
}

async function findCsvDirectory(extractDir: string): Promise<string> {
	const nestedZipPath = await findNestedZip(extractDir, 'fixed_pc_coverage');
	if (!nestedZipPath) {
		throw new Error('Could not find nested postcode coverage ZIP file');
	}

	console.log(`Found nested ZIP: ${nestedZipPath}`);
	console.log('Extracting nested postcode data...');

	const nestedExtractDir = join(extractDir, 'postcode_data');
	await mkdir(nestedExtractDir, { recursive: true });
	await extractZip(nestedZipPath, nestedExtractDir);
	console.log('Nested extraction complete.\n');

	async function findCsvDir(dir: string, depth = 0): Promise<string | null> {
		if (depth > 5) return null;

		const entries = await readdir(dir, { withFileTypes: true });

		const hasCsvFiles = entries.some((e) => e.isFile() && e.name.endsWith('.csv'));
		if (hasCsvFiles) {
			return dir;
		}

		for (const entry of entries) {
			if (entry.isDirectory()) {
				if (entry.name === 'postcode_res_files') {
					return join(dir, entry.name);
				}
				const found = await findCsvDir(join(dir, entry.name), depth + 1);
				if (found) return found;
			}
		}
		return null;
	}

	const csvDir = await findCsvDir(nestedExtractDir);
	if (csvDir) return csvDir;

	throw new Error('Could not find CSV files in extracted postcode data');
}

async function buildDatabase(sourceDir: string): Promise<void> {
	console.log('Building broadband database from Ofcom data...');
	console.log(`Source: ${sourceDir}`);
	console.log(`Output: ${DB_PATH}\n`);

	const db = new Database(DB_PATH, { create: true });

	db.exec(`
		DROP TABLE IF EXISTS postcodes;
		DROP TABLE IF EXISTS outward_aggregates;
		DROP TABLE IF EXISTS area_aggregates;

		CREATE TABLE postcodes (
			postcode TEXT PRIMARY KEY,
			postcode_display TEXT,
			outward TEXT,
			area TEXT,
			pct_under_2mbps REAL,
			pct_2_5mbps REAL,
			pct_5_10mbps REAL,
			pct_10_30mbps REAL,
			pct_30_300mbps REAL,
			pct_over_300mbps REAL,
			sfbb_availability REAL,
			ufbb_100_availability REAL,
			ufbb_availability REAL,
			gigabit_availability REAL,
			nga_availability REAL,
			pct_below_uso REAL,
			pct_unable_2mbps REAL,
			pct_unable_30mbps REAL
		);

		CREATE INDEX idx_outward ON postcodes(outward);
		CREATE INDEX idx_area ON postcodes(area);
	`);

	const insertStmt = db.prepare(`
		INSERT INTO postcodes (
			postcode, postcode_display, outward, area,
			pct_under_2mbps, pct_2_5mbps, pct_5_10mbps, pct_10_30mbps,
			pct_30_300mbps, pct_over_300mbps,
			sfbb_availability, ufbb_100_availability, ufbb_availability,
			gigabit_availability, nga_availability,
			pct_below_uso, pct_unable_2mbps, pct_unable_30mbps
		) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
	`);

	const glob = new Glob('*.csv');
	const files = Array.from(glob.scanSync(sourceDir));

	if (files.length === 0) {
		throw new Error(`No CSV files found in ${sourceDir}`);
	}

	let totalRows = 0;
	let fileCount = 0;

	for (const file of files) {
		const filePath = join(sourceDir, file);
		const content = await Bun.file(filePath).text();
		const lines = content.trim().split('\n');

		const insertMany = db.transaction((rows: string[][]) => {
			for (const row of rows) {
				insertStmt.run(...row);
			}
		});

		const batch: string[][] = [];

		for (let i = 1; i < lines.length; i++) {
			const cols = lines[i]?.split(',');
			if (!cols || cols.length < 19) continue;

			const postcode = cols[0]?.trim().toUpperCase() ?? '';
			const postcodeDisplay = cols[1]?.trim().toUpperCase() ?? '';
			const area = cols[2]?.trim().toUpperCase() ?? '';
			const outward = extractOutward(postcodeDisplay);

			batch.push([
				postcode,
				postcodeDisplay,
				outward,
				area,
				safeParseFloat(cols[5] ?? '').toString(),
				safeParseFloat(cols[6] ?? '').toString(),
				safeParseFloat(cols[7] ?? '').toString(),
				safeParseFloat(cols[8] ?? '').toString(),
				safeParseFloat(cols[3] ?? '').toString(),
				safeParseFloat(cols[4] ?? '').toString(),
				safeParseFloat(cols[9] ?? '').toString(),
				safeParseFloat(cols[10] ?? '').toString(),
				safeParseFloat(cols[11] ?? '').toString(),
				safeParseFloat(cols[16] ?? '').toString(),
				safeParseFloat(cols[18] ?? '').toString(),
				safeParseFloat(cols[17] ?? '').toString(),
				safeParseFloat(cols[12] ?? '').toString(),
				safeParseFloat(cols[15] ?? '').toString(),
			]);

			if (batch.length >= 1000) {
				insertMany(batch);
				totalRows += batch.length;
				batch.length = 0;
			}
		}

		if (batch.length > 0) {
			insertMany(batch);
			totalRows += batch.length;
		}

		fileCount++;
		progress(`Processed ${fileCount}/${files.length} files (${totalRows.toLocaleString()} rows)`);
	}

	progressDone();
	console.log('\nBuilding aggregates...');

	db.exec(`
		CREATE TABLE outward_aggregates AS
		SELECT
			outward,
			COUNT(*) as postcode_count,
			ROUND(AVG(pct_over_300mbps), 1) as avg_pct_over_300mbps,
			ROUND(AVG(gigabit_availability), 1) as avg_gigabit_availability,
			ROUND(AVG(sfbb_availability), 1) as avg_sfbb_availability,
			ROUND(MIN(pct_over_300mbps), 1) as min_pct_over_300mbps,
			ROUND(MAX(pct_over_300mbps), 1) as max_pct_over_300mbps
		FROM postcodes
		GROUP BY outward;

		CREATE UNIQUE INDEX idx_outward_agg ON outward_aggregates(outward);

		CREATE TABLE area_aggregates AS
		SELECT
			area,
			COUNT(*) as postcode_count,
			ROUND(AVG(pct_over_300mbps), 1) as avg_pct_over_300mbps,
			ROUND(AVG(gigabit_availability), 1) as avg_gigabit_availability,
			ROUND(AVG(sfbb_availability), 1) as avg_sfbb_availability,
			ROUND(MIN(pct_over_300mbps), 1) as min_pct_over_300mbps,
			ROUND(MAX(pct_over_300mbps), 1) as max_pct_over_300mbps
		FROM postcodes
		GROUP BY area;

		CREATE UNIQUE INDEX idx_area_agg ON area_aggregates(area);
	`);

	console.log('Optimizing database...');
	db.exec('VACUUM');
	db.exec('ANALYZE');

	const stats = db.query('SELECT COUNT(*) as count FROM postcodes').get() as { count: number };
	const outwardStats = db.query('SELECT COUNT(*) as count FROM outward_aggregates').get() as { count: number };
	const areaStats = db.query('SELECT COUNT(*) as count FROM area_aggregates').get() as { count: number };

	console.log('\nDatabase built successfully!');
	console.log(`  Postcodes: ${stats.count.toLocaleString()}`);
	console.log(`  Outward codes: ${outwardStats.count.toLocaleString()}`);
	console.log(`  Areas: ${areaStats.count.toLocaleString()}`);
	console.log(`  Location: ${DB_PATH}`);

	db.close();
}

export async function build(): Promise<void> {
	await withTempDir('broadband', async (tempDir) => {
		const zipPath = join(tempDir, 'broadband.zip');
		console.log('Downloading Ofcom broadband data...');
		console.log(`URL: ${OFCOM_ZIP_URL}\n`);
		await downloadFile(OFCOM_ZIP_URL, zipPath);
		console.log('Download complete.\n');

		const extractDir = join(tempDir, 'extracted');
		console.log('Extracting ZIP archive...');
		await extractZip(zipPath, extractDir);
		console.log('Extraction complete.\n');

		const csvDir = await findCsvDirectory(extractDir);
		console.log(`Found CSV directory: ${csvDir}\n`);

		await buildDatabase(csvDir);
	});
}

if (import.meta.main) {
	build().catch((err) => {
		console.error('Build failed:', err);
		process.exit(1);
	});
}
