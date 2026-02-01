/**
 * Build postcode lookup database from ONSPD
 *
 * Usage: bun run sources/builders/postcodes.ts
 */

/* biome-ignore-all lint/suspicious/noConsole: Build script uses console for progress */
/* biome-ignore-all lint/complexity/noExcessiveCognitiveComplexity: Build script has inherent complexity */

import { Database } from 'bun:sqlite';
import { createReadStream, mkdirSync } from 'node:fs';
import { join } from 'node:path';
import { createInterface } from 'node:readline';
import { Glob } from 'bun';
import { cleanHeader, createBatchInserter, downloadFile, extractZip, normalizePostcode, parseCsvLine, progress, progressDone, withTempDir } from '../utils/index.ts';

/**
 * Source page: https://geoportal.statistics.gov.uk/datasets/3be72478d8454b59bb86ba97b4ee325b/about
 * Direct download: https://www.arcgis.com/sharing/rest/content/items/3be72478d8454b59bb86ba97b4ee325b/data
 * Notes:
 * - Alternative hosted CSV (no ZIP): https://open-geography-portalx-ons.hub.arcgis.com/api/download/v1/items/cfd03a224ae24db483f89051c35dac29/csv?layers=0
 */
const ONSPD_ZIP_URL = 'https://www.arcgis.com/sharing/rest/content/items/3be72478d8454b59bb86ba97b4ee325b/data';
const DB_PATH = join(import.meta.dirname, '..', 'db', 'postcodes.db');
mkdirSync(join(import.meta.dirname, '..', 'db'), { recursive: true });

async function findCsvFile(extractDir: string): Promise<string> {
	const glob = new Glob('**/ONSPD_*_UK.csv');
	const files = Array.from(glob.scanSync(extractDir));
	if (files.length === 0) {
		throw new Error('Could not find ONSPD CSV in extracted archive');
	}
	const filePath = files[0] ? join(extractDir, files[0]) : '';
	if (!filePath) throw new Error('Missing ONSPD CSV file path');
	return filePath;
}

type HeaderIndex = {
	postcode: number;
	postcodeDisplay: number;
	lat: number;
	lng: number;
	lsoaCode: number;
	lsoaName: number;
	msoaCode: number;
	msoaName: number;
	countryCode: number;
};

function resolveHeaderIndexes(headerLine: string): HeaderIndex {
	const headers = parseCsvLine(headerLine).map(cleanHeader);
	const headerMap = new Map(headers.map((name, idx) => [name, idx]));

	const pickExact = (candidates: string[]): number => {
		for (const name of candidates) {
			const idx = headerMap.get(name.toLowerCase());
			if (idx !== undefined) return idx;
		}
		return -1;
	};

	const pickContains = (patterns: string[]): number => {
		for (const pattern of patterns) {
			const idx = headers.findIndex((h) => h.includes(pattern));
			if (idx >= 0) return idx;
		}
		return -1;
	};

	const postcodeDisplay = pickExact(['pcds', 'pcd']) !== -1 ? pickExact(['pcds', 'pcd']) : pickContains(['postcode (8 char)', 'postcode (7 char)', 'postcode']);
	const postcode = pickExact(['pcd2', 'pcd', 'pcds']) !== -1 ? pickExact(['pcd2', 'pcd', 'pcds']) : pickContains(['postcode (7 char)', 'postcode (8 char)', 'postcode']);
	const lat = pickExact(['lat', 'latitude']) !== -1 ? pickExact(['lat', 'latitude']) : pickContains(['latitude']);
	const lng = pickExact(['long', 'longitude']) !== -1 ? pickExact(['long', 'longitude']) : pickContains(['longitude']);
	const lsoaCode =
		pickExact(['lsoa21cd', 'lsoa11cd', 'lsoa01cd', 'lsoa21', 'lsoa11', 'lsoa01']) !== -1
			? pickExact(['lsoa21cd', 'lsoa11cd', 'lsoa01cd', 'lsoa21', 'lsoa11', 'lsoa01'])
			: pickContains(['lower layer super output area code (2021)', 'lower layer super output area code (2011)', 'lower layer super output area code']);
	const lsoaName = pickExact(['lsoa21nm', 'lsoa11nm', 'lsoa01nm']) !== -1 ? pickExact(['lsoa21nm', 'lsoa11nm', 'lsoa01nm']) : pickContains(['lower layer super output area name']);
	const msoaCode =
		pickExact(['msoa21cd', 'msoa11cd', 'msoa01cd', 'msoa21', 'msoa11', 'msoa01']) !== -1
			? pickExact(['msoa21cd', 'msoa11cd', 'msoa01cd', 'msoa21', 'msoa11', 'msoa01'])
			: pickContains(['middle layer super output area code (2021)', 'middle layer super output area code (2011)', 'middle layer super output area code']);
	const msoaName = pickExact(['msoa21nm', 'msoa11nm', 'msoa01nm']) !== -1 ? pickExact(['msoa21nm', 'msoa11nm', 'msoa01nm']) : pickContains(['middle layer super output area name']);
	const countryCode = pickExact(['ctry', 'ctry21cd', 'ctry11cd']) !== -1 ? pickExact(['ctry', 'ctry21cd', 'ctry11cd']) : pickContains(['country code']);

	if (postcodeDisplay === -1 || lat === -1 || lng === -1 || lsoaCode === -1 || msoaCode === -1) {
		throw new Error('Required ONSPD columns not found in header');
	}

	return { postcode, postcodeDisplay, lat, lng, lsoaCode, lsoaName, msoaCode, msoaName, countryCode };
}

async function buildDatabase(csvPath: string): Promise<void> {
	console.log('Building postcodes database...');
	console.log(`Source: ${csvPath}`);
	console.log(`Output: ${DB_PATH}\n`);

	const db = new Database(DB_PATH, { create: true });
	db.exec(`
		DROP TABLE IF EXISTS postcodes;
		CREATE TABLE postcodes (
			postcode TEXT PRIMARY KEY,
			postcode_display TEXT,
			lat REAL,
			lng REAL,
			lsoa_code TEXT,
			lsoa_name TEXT,
			msoa_code TEXT,
			msoa_name TEXT,
			country_code TEXT
		);
		CREATE INDEX idx_postcodes_lsoa ON postcodes(lsoa_code);
		CREATE INDEX idx_postcodes_msoa ON postcodes(msoa_code);
	`);

	const inserter = createBatchInserter<[string, string, number | null, number | null, string | null, string | null, string | null, string | null, string | null]>(
		db,
		'INSERT INTO postcodes (postcode, postcode_display, lat, lng, lsoa_code, lsoa_name, msoa_code, msoa_name, country_code) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)',
		{
			batchSize: 10000,
			onProgress: (n) => progress(`Processed ${n.toLocaleString()} rows`),
		},
	);

	const stream = createReadStream(csvPath, { encoding: 'utf-8' });
	const rl = createInterface({ input: stream, crlfDelay: Infinity });

	let headerParsed = false;
	let headerIndex: HeaderIndex | null = null;

	for await (const line of rl) {
		if (!headerParsed) {
			headerIndex = resolveHeaderIndexes(line);
			headerParsed = true;
			continue;
		}
		if (!headerIndex) continue;

		const cols = parseCsvLine(line);
		const postcodeDisplay = cols[headerIndex.postcodeDisplay] ?? '';
		if (!postcodeDisplay) continue;

		const postcode = normalizePostcode(postcodeDisplay);
		const latRaw = cols[headerIndex.lat];
		const lngRaw = cols[headerIndex.lng];
		const lat = latRaw ? Number.parseFloat(latRaw) : null;
		const lng = lngRaw ? Number.parseFloat(lngRaw) : null;
		const lsoaCode = cols[headerIndex.lsoaCode] ?? null;
		const lsoaName = headerIndex.lsoaName >= 0 ? (cols[headerIndex.lsoaName] ?? null) : null;
		const msoaCode = cols[headerIndex.msoaCode] ?? null;
		const msoaName = headerIndex.msoaName >= 0 ? (cols[headerIndex.msoaName] ?? null) : null;
		const countryCode = headerIndex.countryCode >= 0 ? (cols[headerIndex.countryCode] ?? null) : null;

		inserter.add([
			postcode,
			postcodeDisplay.trim().toUpperCase(),
			Number.isNaN(lat ?? NaN) ? null : lat,
			Number.isNaN(lng ?? NaN) ? null : lng,
			lsoaCode?.trim() ?? null,
			lsoaName?.trim() ?? null,
			msoaCode?.trim() ?? null,
			msoaName?.trim() ?? null,
			countryCode?.trim() ?? null,
		]);
	}

	const totalRows = inserter.flush();
	progressDone();
	console.log(`Inserted ${totalRows.toLocaleString()} postcodes.`);
	db.close();
}

export async function build(): Promise<void> {
	await withTempDir('onspd', async (tempDir) => {
		const zipPath = join(tempDir, 'onspd.zip');
		console.log('Downloading ONSPD...');
		console.log(`URL: ${ONSPD_ZIP_URL}\n`);
		await downloadFile(ONSPD_ZIP_URL, zipPath);
		console.log('Download complete.\n');

		const extractDir = join(tempDir, 'extract');
		console.log('Extracting ZIP archive...');
		await extractZip(zipPath, extractDir);
		console.log('Extraction complete.\n');

		const csvPath = await findCsvFile(extractDir);
		await buildDatabase(csvPath);
	});
}

if (import.meta.main) {
	build().catch((err) => {
		console.error('Postcodes build failed:', err);
		process.exit(1);
	});
}
