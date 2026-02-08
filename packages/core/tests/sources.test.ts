/**
 * Source database schema validation tests
 *
 * Verifies each source database has the expected table and columns.
 * Skipped by default — run with: BUN_TEST_SOURCES=1 bun test sources.test.ts
 * Or use: bun run test:sources
 *
 * Each suite is individually skipped if its .db file doesn't exist.
 * Run `bun run sources:build` to build all databases.
 */

import { Database } from 'bun:sqlite';
import { afterAll, describe, expect, test } from 'bun:test';
import { existsSync } from 'node:fs';
import { paths } from '@let/core/paths';

const SOURCES_ENABLED = process.env['BUN_TEST_SOURCES'] === '1';
const p = paths();
const sourceDb = (name: string) => p.derived.sourceDb(name);

/** Open a read-only database, track for cleanup */
const openDbs: Database[] = [];
function open(name: string): Database {
	const db = new Database(sourceDb(name), { readonly: true });
	openDbs.push(db);
	return db;
}

afterAll(() => {
	for (const db of openDbs) db.close();
});

function dbAvailable(name: string): boolean {
	return SOURCES_ENABLED && existsSync(sourceDb(name));
}

/** Get column names for a table */
function columnNames(db: Database, table: string): string[] {
	const rows = db.query(`PRAGMA table_info(${table})`).all() as { name: string }[];
	return rows.map((r) => r.name);
}

// ---------------------------------------------------------------------------
// Postcodes
// ---------------------------------------------------------------------------

describe.skipIf(!dbAvailable('postcodes'))('postcodes.db', () => {
	test('schema has expected columns and rows', () => {
		const db = open('postcodes');
		const cols = columnNames(db, 'postcodes');
		for (const col of ['postcode', 'lat', 'lng', 'lsoa_code', 'lsoa_name', 'msoa_code', 'msoa_name']) {
			expect(cols).toContain(col);
		}
		const { cnt } = db.query('SELECT COUNT(*) as cnt FROM postcodes').get() as { cnt: number };
		expect(cnt).toBeGreaterThan(2_000_000);
	});
});

// ---------------------------------------------------------------------------
// Deprivation
// ---------------------------------------------------------------------------

describe.skipIf(!dbAvailable('deprivation'))('deprivation.db', () => {
	test('schema has expected columns and rows', () => {
		const db = open('deprivation');
		const cols = columnNames(db, 'imd');
		for (const col of ['lsoa_code', 'rank', 'decile', 'score']) {
			expect(cols).toContain(col);
		}
		const { cnt } = db.query('SELECT COUNT(*) as cnt FROM imd').get() as { cnt: number };
		expect(cnt).toBeGreaterThan(30_000);
	});
});

// ---------------------------------------------------------------------------
// Census tenure
// ---------------------------------------------------------------------------

describe.skipIf(!dbAvailable('census'))('census.db', () => {
	test('schema has expected columns and rows', () => {
		const db = open('census');
		const cols = columnNames(db, 'tenure');
		for (const col of ['lsoa_code', 'total_households', 'social_housing_pct']) {
			expect(cols).toContain(col);
		}
		const { cnt } = db.query('SELECT COUNT(*) as cnt FROM tenure').get() as { cnt: number };
		expect(cnt).toBeGreaterThan(30_000);
	});
});

// ---------------------------------------------------------------------------
// Population
// ---------------------------------------------------------------------------

describe.skipIf(!dbAvailable('population'))('population.db', () => {
	test('schema has expected columns and rows', () => {
		const db = open('population');
		const cols = columnNames(db, 'population');
		for (const col of ['lsoa_code', 'population']) {
			expect(cols).toContain(col);
		}
		const { cnt } = db.query('SELECT COUNT(*) as cnt FROM population').get() as { cnt: number };
		expect(cnt).toBeGreaterThan(30_000);
	});
});

// ---------------------------------------------------------------------------
// Income
// ---------------------------------------------------------------------------

describe.skipIf(!dbAvailable('income'))('income.db', () => {
	test('schema has expected columns and rows', () => {
		const db = open('income');
		const cols = columnNames(db, 'income');
		for (const col of ['msoa_code', 'income_bhc', 'income_ahc']) {
			expect(cols).toContain(col);
		}
		const { cnt } = db.query('SELECT COUNT(*) as cnt FROM income').get() as { cnt: number };
		expect(cnt).toBeGreaterThan(7_000);
	});
});

// ---------------------------------------------------------------------------
// NaPTAN stops
// ---------------------------------------------------------------------------

describe.skipIf(!dbAvailable('naptan'))('naptan.db', () => {
	test('schema has expected columns and rows', () => {
		const db = open('naptan');
		const cols = columnNames(db, 'stops');
		for (const col of ['atco_code', 'common_name', 'stop_type', 'lat', 'lng']) {
			expect(cols).toContain(col);
		}
		const { cnt } = db.query('SELECT COUNT(*) as cnt FROM stops').get() as { cnt: number };
		expect(cnt).toBeGreaterThan(400_000);
	});
});

// ---------------------------------------------------------------------------
// Flood risk
// ---------------------------------------------------------------------------

describe.skipIf(!dbAvailable('flood'))('flood.db', () => {
	test('schema has expected columns and rows', () => {
		const db = open('flood');
		const cols = columnNames(db, 'flood');
		for (const col of ['postcode', 'risk', 'source']) {
			expect(cols).toContain(col);
		}
		const { cnt } = db.query('SELECT COUNT(*) as cnt FROM flood').get() as { cnt: number };
		expect(cnt).toBeGreaterThan(100_000);
	});
});

// ---------------------------------------------------------------------------
// Crime
// ---------------------------------------------------------------------------

describe.skipIf(!dbAvailable('crime'))('crime.db', () => {
	test('schema has expected columns and rows', () => {
		const db = open('crime');
		const cols = columnNames(db, 'crime_12m');
		for (const col of ['lsoa_code', 'total', 'violent', 'burglary', 'robbery']) {
			expect(cols).toContain(col);
		}
		const { cnt } = db.query('SELECT COUNT(*) as cnt FROM crime_12m').get() as { cnt: number };
		expect(cnt).toBeGreaterThan(10_000);
	});
});

// ---------------------------------------------------------------------------
// Broadband
// ---------------------------------------------------------------------------

describe.skipIf(!dbAvailable('broadband'))('broadband.db', () => {
	test('schema has expected tables, columns, and rows', () => {
		const db = open('broadband');
		const postcodeCols = columnNames(db, 'postcodes');
		for (const col of ['postcode', 'outward', 'area', 'gigabit_availability']) {
			expect(postcodeCols).toContain(col);
		}
		const outwardCols = columnNames(db, 'outward_aggregates');
		for (const col of ['outward', 'avg_gigabit_availability']) {
			expect(outwardCols).toContain(col);
		}
		const areaCols = columnNames(db, 'area_aggregates');
		for (const col of ['area', 'avg_gigabit_availability']) {
			expect(areaCols).toContain(col);
		}
		const { cnt } = db.query('SELECT COUNT(*) as cnt FROM postcodes').get() as { cnt: number };
		expect(cnt).toBeGreaterThan(1_000_000);
	});
});

// ---------------------------------------------------------------------------
// UPRN
// ---------------------------------------------------------------------------

describe.skipIf(!dbAvailable('uprn'))('uprn.db', () => {
	test('schema has expected columns and rows', () => {
		const db = open('uprn');
		const cols = columnNames(db, 'uprn');
		for (const col of ['uprn', 'lat', 'lng']) {
			expect(cols).toContain(col);
		}
		const { cnt } = db.query('SELECT COUNT(*) as cnt FROM uprn').get() as { cnt: number };
		expect(cnt).toBeGreaterThan(1_000_000);
	});
});
