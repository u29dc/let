/**
 * Build crime statistics database from Police.uk archive
 *
 * Usage: bun run sources/builders/crime.ts
 * Optional: CRIME_ARCHIVE_PATH=/path/to/latest.zip
 */

/* biome-ignore-all lint/suspicious/noConsole: Build script uses console for progress */
/* biome-ignore-all lint/complexity/noExcessiveCognitiveComplexity: Large dataset processing */

import { Database } from 'bun:sqlite';
import { createReadStream, mkdirSync } from 'node:fs';
import { join } from 'node:path';
import { createInterface } from 'node:readline';
import { Glob } from 'bun';
import { createBatchInserter, downloadFile, extractZip, findColumnIndex, isTTY, parseCsvLine, progress, progressDone, withTempDir } from '../utils/index.ts';

/**
 * Source page: https://data.police.uk/data/
 * Direct download: https://data.police.uk/data/archive/latest.zip
 * Notes:
 * - Optional override: CRIME_ARCHIVE_PATH points to a local ZIP.
 */
const CRIME_ZIP_URL = 'https://data.police.uk/data/archive/latest.zip';
const DB_PATH = join(import.meta.dirname, '..', 'db', 'crime.db');
mkdirSync(join(import.meta.dirname, '..', 'db'), { recursive: true });

type CrimeCounts = { total: number; violent: number; burglary: number; robbery: number };

function incrementCounts(target: CrimeCounts, type: string): void {
	target.total += 1;
	const lower = type.toLowerCase();
	if (lower.includes('violence')) target.violent += 1;
	if (lower.includes('burglary')) target.burglary += 1;
	if (lower.includes('robbery')) target.robbery += 1;
}

async function buildDatabase(extractDir: string): Promise<void> {
	console.log('Building crime database...');

	const glob = new Glob('**/*-street.csv');
	const files = Array.from(glob.scanSync(extractDir));
	if (files.length === 0) {
		throw new Error('No street crime CSV files found');
	}

	const monthlyCounts = new Map<string, CrimeCounts>();
	const months = new Set<string>();

	let fileIndex = 0;
	let processedRows = 0;

	for (const relPath of files) {
		fileIndex += 1;
		const filePath = join(extractDir, relPath);
		if (isTTY) console.log(`Processing ${fileIndex}/${files.length}: ${relPath}`);

		const stream = createReadStream(filePath, { encoding: 'utf-8' });
		const rl = createInterface({ input: stream, crlfDelay: Infinity });

		let headerParsed = false;
		let monthIdx = -1;
		let lsoaIdx = -1;
		let typeIdx = -1;

		for await (const line of rl) {
			if (!headerParsed) {
				const header = parseCsvLine(line);
				monthIdx = findColumnIndex(header, ['month']);
				lsoaIdx = findColumnIndex(header, ['lsoa code']);
				typeIdx = findColumnIndex(header, ['crime type']);
				if (monthIdx < 0 || lsoaIdx < 0 || typeIdx < 0) {
					throw new Error(`Required columns not found in ${relPath}`);
				}
				headerParsed = true;
				continue;
			}

			const cols = parseCsvLine(line);
			const month = cols[monthIdx]?.trim();
			const lsoa = cols[lsoaIdx]?.trim();
			const crimeType = cols[typeIdx]?.trim() ?? '';
			if (!month || !lsoa) continue;

			months.add(month);
			const key = `${lsoa}|${month}`;
			const existing = monthlyCounts.get(key) ?? { total: 0, violent: 0, burglary: 0, robbery: 0 };
			incrementCounts(existing, crimeType);
			monthlyCounts.set(key, existing);
			processedRows += 1;
		}
		progress(`Processed ${processedRows.toLocaleString()} rows`);
		progressDone();
	}

	console.log('Aggregating 12-month totals...');
	const monthList = Array.from(months).sort();
	const last12 = monthList.slice(-12);
	const last12Set = new Set(last12);

	const twelveMonthTotals = new Map<string, CrimeCounts>();
	for (const [key, counts] of monthlyCounts.entries()) {
		const [lsoa, month] = key.split('|');
		if (!lsoa || !month) continue;
		if (!last12Set.has(month)) continue;
		const existing = twelveMonthTotals.get(lsoa) ?? { total: 0, violent: 0, burglary: 0, robbery: 0 };
		existing.total += counts.total;
		existing.violent += counts.violent;
		existing.burglary += counts.burglary;
		existing.robbery += counts.robbery;
		twelveMonthTotals.set(lsoa, existing);
	}

	console.log(`Writing database: ${DB_PATH}`);
	const db = new Database(DB_PATH, { create: true });
	db.exec(`
		DROP TABLE IF EXISTS crime_monthly;
		DROP TABLE IF EXISTS crime_12m;

		CREATE TABLE crime_monthly (
			lsoa_code TEXT NOT NULL,
			month TEXT NOT NULL,
			total INTEGER,
			violent INTEGER,
			burglary INTEGER,
			robbery INTEGER,
			PRIMARY KEY (lsoa_code, month)
		);
		CREATE INDEX idx_crime_monthly_lsoa ON crime_monthly(lsoa_code);
		CREATE INDEX idx_crime_monthly_month ON crime_monthly(month);

		CREATE TABLE crime_12m (
			lsoa_code TEXT PRIMARY KEY,
			total INTEGER,
			violent INTEGER,
			burglary INTEGER,
			robbery INTEGER,
			month_start TEXT,
			month_end TEXT
		);
	`);

	const monthlyInserter = createBatchInserter<[string, string, number, number, number, number]>(
		db,
		'INSERT INTO crime_monthly (lsoa_code, month, total, violent, burglary, robbery) VALUES (?, ?, ?, ?, ?, ?)',
		{ batchSize: 10000 },
	);

	for (const [key, counts] of monthlyCounts.entries()) {
		const [lsoa, month] = key.split('|');
		if (!lsoa || !month) continue;
		monthlyInserter.add([lsoa, month, counts.total, counts.violent, counts.burglary, counts.robbery]);
	}
	const monthlyInserted = monthlyInserter.flush();

	const monthStart = last12[0] ?? null;
	const monthEnd = last12[last12.length - 1] ?? null;

	const twelveMInserter = createBatchInserter<[string, number, number, number, number, string | null, string | null]>(
		db,
		'INSERT INTO crime_12m (lsoa_code, total, violent, burglary, robbery, month_start, month_end) VALUES (?, ?, ?, ?, ?, ?, ?)',
		{ batchSize: 10000 },
	);

	for (const [lsoa, counts] of twelveMonthTotals.entries()) {
		twelveMInserter.add([lsoa, counts.total, counts.violent, counts.burglary, counts.robbery, monthStart, monthEnd]);
	}
	const total12 = twelveMInserter.flush();

	db.close();
	console.log(`Inserted ${monthlyInserted.toLocaleString()} monthly rows and ${total12.toLocaleString()} 12m rows.`);
}

export async function build(): Promise<void> {
	await withTempDir('crime', async (tempDir) => {
		const localPath = process.env['CRIME_ARCHIVE_PATH'];
		let zipPath: string;

		if (localPath) {
			console.log('Using local crime archive...');
			console.log(`Path: ${localPath}\n`);
			zipPath = localPath;
		} else {
			zipPath = join(tempDir, 'crime.zip');
			console.log('Downloading Police.uk archive...');
			console.log(`URL: ${CRIME_ZIP_URL}\n`);
			await downloadFile(CRIME_ZIP_URL, zipPath);
			console.log('Download complete.\n');
		}

		const extractDir = join(tempDir, 'extract');
		console.log('Extracting ZIP archive...');
		await extractZip(zipPath, extractDir);
		console.log('Extraction complete.\n');

		await buildDatabase(extractDir);
	});
}

if (import.meta.main) {
	build().catch((err) => {
		console.error('Crime build failed:', err);
		process.exit(1);
	});
}
