/**
 * Build income estimates database (MSOA)
 *
 * Usage: bun run scripts/sources/income.ts
 */

/* biome-ignore-all lint/suspicious/noConsole: Build script uses console for progress */

import { Database } from 'bun:sqlite';
import { mkdirSync } from 'node:fs';
import { join } from 'node:path';
import * as XLSX from 'xlsx';
import { createBatchInserter, downloadFile, SOURCES_DIR, withTempDir } from '../utils.ts';

/**
 * Source page: https://www.ons.gov.uk/peoplepopulationandcommunity/personalandhouseholdfinances/incomeandwealth/bulletins/smallareamodelbasedincomeestimates/financialyearending2023
 * Direct download: https://www.ons.gov.uk/visualisations/dvc3434/fig01/datadownload.xlsx?
 * Notes:
 * - Some endpoints return 403 without a browser User-Agent.
 * - Overrides supported via INCOME_XLSX_URL or INCOME_XLSX_PATH.
 */
const INCOME_XLSX_URL = 'https://www.ons.gov.uk/visualisations/dvc3434/fig01/datadownload.xlsx?';
const DB_PATH = join(SOURCES_DIR, 'income.db');
mkdirSync(SOURCES_DIR, { recursive: true });

async function downloadDataset(tempDir: string): Promise<string> {
	const xlsxPath = join(tempDir, 'income.xlsx');
	const overridePath = process.env['INCOME_XLSX_PATH'];
	if (overridePath) {
		console.log('Using local income dataset...');
		console.log(`Path: ${overridePath}\n`);
		return overridePath;
	}

	console.log('Downloading income estimates (Excel)...');
	const downloadUrl = process.env['INCOME_XLSX_URL'] ?? INCOME_XLSX_URL;
	console.log(`URL: ${downloadUrl}\n`);
	await downloadFile(downloadUrl, xlsxPath, {
		headers: {
			'User-Agent': 'Mozilla/5.0',
			Accept: 'application/vnd.openxmlformats-officedocument.spreadsheetml.sheet',
		},
	});
	console.log('Download complete.\n');
	return xlsxPath;
}

function normalizeHeaderRow(row: unknown[]): string[] {
	return row.map((cell) => (typeof cell === 'string' ? cell.trim() : String(cell ?? '')).trim());
}

function findHeaderRow(rows: unknown[][]): { header: string[]; index: number } {
	for (let i = 0; i < rows.length; i++) {
		const row = normalizeHeaderRow(rows[i] ?? []);
		const joined = row.join(' ').toLowerCase();
		if ((joined.includes('msoa') && joined.includes('income')) || (joined.includes('areacd') && joined.includes('income'))) {
			return { header: row, index: i };
		}
	}
	throw new Error('Could not locate header row in income workbook');
}

function findIndex(headers: string[], includesAll: string[], excludes: string[] = []): number {
	const lowered = headers.map((h) => h.toLowerCase());
	for (let i = 0; i < lowered.length; i++) {
		const lower = lowered[i] ?? '';
		if (includesAll.every((term) => lower.includes(term)) && excludes.every((term) => !lower.includes(term))) {
			return i;
		}
	}
	return -1;
}

interface ColumnIndices {
	msoaCode: number;
	msoaName: number;
	mean: number;
	median: number;
}

function resolveColumnIndices(header: string[]): ColumnIndices {
	let msoaCode = findIndex(header, ['msoa', 'code']);
	if (msoaCode < 0) msoaCode = findIndex(header, ['areacd']);

	const msoaName = findIndex(header, ['msoa', 'name']);

	let mean = findIndex(header, ['mean', 'income'], ['lower', 'upper', 'ci']);
	if (mean < 0) mean = findIndex(header, ['before housing']);

	let median = findIndex(header, ['median', 'income'], ['lower', 'upper', 'ci']);
	if (median < 0) median = findIndex(header, ['after housing']);

	if (msoaCode < 0 || mean < 0 || median < 0) {
		throw new Error('Required income columns not found');
	}

	return { msoaCode, msoaName, mean, median };
}

function parseNumericCell(value: unknown): number | null {
	const num = typeof value === 'number' ? value : Number.parseFloat(String(value ?? ''));
	return Number.isNaN(num) ? null : num;
}

function parseIncomeRow(values: unknown[], cols: ColumnIndices): [string, string | null, number | null, number | null] | null {
	const msoaCode = values[cols.msoaCode];
	if (typeof msoaCode !== 'string' || msoaCode.trim() === '') return null;

	const msoaNameRaw = cols.msoaName >= 0 ? values[cols.msoaName] : null;
	const msoaName = typeof msoaNameRaw === 'string' ? msoaNameRaw.trim() : null;

	return [msoaCode.trim(), msoaName, parseNumericCell(values[cols.mean]), parseNumericCell(values[cols.median])];
}

function readWorksheetRows(xlsxPath: string): unknown[][] {
	const workbook = XLSX.readFile(xlsxPath, { cellDates: false });
	const sheetName = workbook.SheetNames[0];
	if (!sheetName) throw new Error('Excel workbook has no sheets');
	const sheet = workbook.Sheets[sheetName];
	if (!sheet) throw new Error('Missing worksheet data');

	const rows = XLSX.utils.sheet_to_json<unknown[]>(sheet, { defval: null, header: 1 });
	if (rows.length === 0) throw new Error('No rows found in income dataset');
	return rows;
}

async function buildDatabase(xlsxPath: string): Promise<void> {
	console.log('Building income database...');
	console.log(`Source: ${xlsxPath}`);
	console.log(`Output: ${DB_PATH}\n`);

	const rows = readWorksheetRows(xlsxPath);
	const { header, index: headerIndex } = findHeaderRow(rows);
	const dataRows = rows.slice(headerIndex + 1);
	const cols = resolveColumnIndices(header);

	const db = new Database(DB_PATH, { create: true });
	db.exec(`
		DROP TABLE IF EXISTS income;
		CREATE TABLE income (
			msoa_code TEXT PRIMARY KEY,
			msoa_name TEXT,
			income_bhc REAL,
			income_ahc REAL
		);
		CREATE INDEX idx_income_bhc ON income(income_bhc);
		CREATE INDEX idx_income_ahc ON income(income_ahc);
	`);

	const inserter = createBatchInserter<[string, string | null, number | null, number | null]>(db, 'INSERT INTO income (msoa_code, msoa_name, income_bhc, income_ahc) VALUES (?, ?, ?, ?)', {
		batchSize: 5000,
	});

	for (const row of dataRows) {
		const values = Array.isArray(row) ? row : [];
		const parsed = parseIncomeRow(values, cols);
		if (parsed) inserter.add(parsed);
	}

	const totalRows = inserter.flush();
	console.log(`Inserted ${totalRows.toLocaleString()} MSOAs.`);
	db.close();
}

export async function build(): Promise<void> {
	await withTempDir('income', async (tempDir) => {
		const xlsxPath = await downloadDataset(tempDir);
		await buildDatabase(xlsxPath);
	});
}

if (import.meta.main) {
	build().catch((err) => {
		console.error('Income build failed:', err);
		process.exit(1);
	});
}
