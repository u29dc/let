/**
 * Build flood risk lookup database (postcode-based)
 *
 * Usage: bun run sources/builders/flood.ts
 */

/* biome-ignore-all lint/suspicious/noConsole: Build script uses console for progress */

import { Database } from 'bun:sqlite';
import { createReadStream, mkdirSync } from 'node:fs';
import { join } from 'node:path';
import { createInterface } from 'node:readline';
import { checkSasExpiry, createBatchInserter, downloadFile, findColumnIndex, normalizePostcode, parseCsvLine, progress, progressDone, withTempDir } from '../utils/index.ts';

/**
 * Source page: https://environment.data.gov.uk/dataset/53cba123-71f8-417a-8441-4c7ba111e8e1
 * Direct download: https://agrilake2live.file.core.windows.net/gms-datasets/fb921496-1788-4fc2-b469-7b51e2a45553/Postcodes_Risk_Assessment_All.csv?sv=2022-11-02&se=2026-02-09T12%3A34%3A08Z&sr=f&sp=r&sig=ZqHp87BTmcoetaCQ7aVNxBx0Sb5fVjoJEq50vFG0zZY%3D
 * Notes:
 * - SAS links can expire; overrides supported via FLOOD_CSV_URL or FLOOD_CSV_PATH.
 * - Human service reference: https://www.gov.uk/check-long-term-flood-risk
 */
const FLOOD_CSV_URL =
	'https://agrilake2live.file.core.windows.net/gms-datasets/fb921496-1788-4fc2-b469-7b51e2a45553/Postcodes_Risk_Assessment_All.csv?sv=2022-11-02&se=2026-02-09T12%3A34%3A08Z&sr=f&sp=r&sig=ZqHp87BTmcoetaCQ7aVNxBx0Sb5fVjoJEq50vFG0zZY%3D';
const FLOOD_SOURCE_PAGE = 'https://environment.data.gov.uk/dataset/53cba123-71f8-417a-8441-4c7ba111e8e1';
const DB_PATH = join(import.meta.dirname, '..', 'db', 'flood.db');
mkdirSync(join(import.meta.dirname, '..', 'db'), { recursive: true });

async function downloadDataset(tempDir: string): Promise<string> {
	const csvPath = join(tempDir, 'flood.csv');
	const overridePath = process.env['FLOOD_CSV_PATH'];
	if (overridePath) {
		console.log('Using local flood dataset...');
		console.log(`Path: ${overridePath}\n`);
		return overridePath;
	}

	console.log('Downloading flood risk dataset...');
	const downloadUrl = process.env['FLOOD_CSV_URL'] ?? FLOOD_CSV_URL;
	console.log(`URL: ${downloadUrl}\n`);
	checkSasExpiry(downloadUrl, {
		sourceName: 'flood',
		sourcePageUrl: FLOOD_SOURCE_PAGE,
		envUrlVar: 'FLOOD_CSV_URL',
		envPathVar: 'FLOOD_CSV_PATH',
		buildCommand: 'bun run sources:flood',
	});
	await downloadFile(downloadUrl, csvPath);
	console.log('Download complete.\n');
	return csvPath;
}

async function buildDatabase(csvPath: string): Promise<void> {
	console.log('Building flood risk database...');
	console.log(`Source: ${csvPath}`);
	console.log(`Output: ${DB_PATH}\n`);

	const db = new Database(DB_PATH, { create: true });
	db.exec(`
		DROP TABLE IF EXISTS flood;
		CREATE TABLE flood (
			postcode TEXT PRIMARY KEY,
			risk TEXT,
			source TEXT
		);
	`);

	const inserter = createBatchInserter<[string, string | null, string]>(db, 'INSERT INTO flood (postcode, risk, source) VALUES (?, ?, ?)', {
		batchSize: 10000,
		onProgress: (n) => progress(`Processed ${n.toLocaleString()} rows`),
	});

	const stream = createReadStream(csvPath, { encoding: 'utf-8' });
	const rl = createInterface({ input: stream, crlfDelay: Infinity });

	let headerParsed = false;
	let postcodeIdx = -1;
	let riskIdx = -1;

	for await (const line of rl) {
		if (!headerParsed) {
			const header = parseCsvLine(line);
			postcodeIdx = findColumnIndex(header, ['postcode']);
			riskIdx = findColumnIndex(header, ['overall risk', 'risk overall', 'risk category', 'risk']);
			if (postcodeIdx < 0 || riskIdx < 0) {
				throw new Error('Required flood columns not found');
			}
			headerParsed = true;
			continue;
		}

		const cols = parseCsvLine(line);
		const postcodeRaw = cols[postcodeIdx] ?? '';
		if (!postcodeRaw) continue;
		const postcode = normalizePostcode(postcodeRaw);
		const risk = cols[riskIdx]?.trim() ?? null;
		inserter.add([postcode, risk, 'ea-postcode-risk']);
	}

	const totalRows = inserter.flush();
	progressDone();
	console.log(`Inserted ${totalRows.toLocaleString()} postcodes.`);
	db.close();
}

export async function build(): Promise<void> {
	await withTempDir('flood', async (tempDir) => {
		const csvPath = await downloadDataset(tempDir);
		await buildDatabase(csvPath);
	});
}

if (import.meta.main) {
	build().catch((err) => {
		console.error('Flood build failed:', err);
		process.exit(1);
	});
}
