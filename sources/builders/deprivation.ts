/**
 * Build deprivation (IMD 2025) database
 *
 * Usage: bun run sources/builders/deprivation.ts
 */

/* biome-ignore-all lint/suspicious/noConsole: Build script uses console for progress */

import { Database } from 'bun:sqlite';
import { mkdirSync } from 'node:fs';
import { join } from 'node:path';
import { createBatchInserter, downloadFile, findColumnIndex, parseCsvLine, progress, progressDone, toInt, toNumber, withTempDir } from '../utils/index.ts';

/**
 * Source page: https://www.gov.uk/government/statistics/english-indices-of-deprivation-2025
 * Direct download: https://assets.publishing.service.gov.uk/media/691ded56d140bbbaa59a2a7d/File_7_IoD2025_All_Ranks_Scores_Deciles_Population_Denominators.csv
 */
const IMD_CSV_URL = 'https://assets.publishing.service.gov.uk/media/691ded56d140bbbaa59a2a7d/File_7_IoD2025_All_Ranks_Scores_Deciles_Population_Denominators.csv';
const DB_PATH = join(import.meta.dirname, '..', 'db', 'deprivation.db');
mkdirSync(join(import.meta.dirname, '..', 'db'), { recursive: true });

async function buildDatabase(csvPath: string): Promise<void> {
	console.log('Building deprivation database...');
	console.log(`Source: ${csvPath}`);
	console.log(`Output: ${DB_PATH}\n`);

	const content = await Bun.file(csvPath).text();
	const lines = content.split(/\r?\n/).filter(Boolean);
	if (lines.length === 0) throw new Error('Empty IMD CSV');

	const header = parseCsvLine(lines[0] ?? '');
	const lsoaIdx = findColumnIndex(header, ['lsoa code']);
	const rankIdx = findColumnIndex(header, ['index of multiple deprivation (imd) rank', 'imd) rank', 'imd rank']);
	const decileIdx = findColumnIndex(header, ['index of multiple deprivation (imd) decile', 'imd) decile', 'imd decile']);
	const scoreIdx = findColumnIndex(header, ['index of multiple deprivation (imd) score', 'imd) score', 'imd score']);

	if (lsoaIdx < 0 || rankIdx < 0 || decileIdx < 0 || scoreIdx < 0) {
		throw new Error('Required IMD columns not found');
	}

	const db = new Database(DB_PATH, { create: true });
	db.exec(`
		DROP TABLE IF EXISTS imd;
		CREATE TABLE imd (
			lsoa_code TEXT PRIMARY KEY,
			rank INTEGER,
			decile INTEGER,
			score REAL
		);
		CREATE INDEX idx_imd_rank ON imd(rank);
		CREATE INDEX idx_imd_decile ON imd(decile);
	`);

	const inserter = createBatchInserter<[string, number | null, number | null, number | null]>(db, 'INSERT INTO imd (lsoa_code, rank, decile, score) VALUES (?, ?, ?, ?)', {
		batchSize: 5000,
		onProgress: (n) => progress(`Inserted ${n.toLocaleString()} rows`),
	});

	for (let i = 1; i < lines.length; i++) {
		const cols = parseCsvLine(lines[i] ?? '');
		const lsoa = cols[lsoaIdx]?.trim();
		if (!lsoa) continue;
		inserter.add([lsoa, toInt(cols[rankIdx]), toInt(cols[decileIdx]), toNumber(cols[scoreIdx])]);
	}

	const totalRows = inserter.flush();
	progressDone();
	console.log(`Inserted ${totalRows.toLocaleString()} LSOAs.`);
	db.close();
}

export async function build(): Promise<void> {
	await withTempDir('imd', async (tempDir) => {
		const csvPath = join(tempDir, 'imd.csv');
		console.log('Downloading IMD 2025 File 7...');
		console.log(`URL: ${IMD_CSV_URL}\n`);
		await downloadFile(IMD_CSV_URL, csvPath);
		console.log('Download complete.\n');
		await buildDatabase(csvPath);
	});
}

if (import.meta.main) {
	build().catch((err) => {
		console.error('Deprivation build failed:', err);
		process.exit(1);
	});
}
