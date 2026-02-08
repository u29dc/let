/**
 * Source database schema validation tests
 *
 * Verifies each source database has the expected table and columns.
 * Databases that don't exist are skipped with a log message.
 * Run `bun run sources:build` to build all databases.
 */

import { Database } from 'bun:sqlite';
import { afterAll, describe, expect, test } from 'bun:test';
import { existsSync } from 'node:fs';
import { paths } from '@let/core/paths';

const p = paths();
const sourceDb = (name: string) => p.derived.sourceDb(name);

const openDbs: Database[] = [];
function open(name: string): Database {
	const db = new Database(sourceDb(name), { readonly: true });
	openDbs.push(db);
	return db;
}

afterAll(() => {
	for (const db of openDbs) db.close();
});

function dbExists(name: string): boolean {
	const exists = existsSync(sourceDb(name));
	// biome-ignore lint/suspicious/noConsole: intentional test skip message
	if (!exists) console.log(`${name}.db not found, skipping`);
	return exists;
}

function columnNames(db: Database, table: string): string[] {
	const rows = db.query(`PRAGMA table_info(${table})`).all() as { name: string }[];
	return rows.map((r) => r.name);
}

// ---------------------------------------------------------------------------

describe.skipIf(!dbExists('postcodes'))('postcodes.db', () => {
	test('schema has expected columns', () => {
		const cols = columnNames(open('postcodes'), 'postcodes');
		for (const col of ['postcode', 'lat', 'lng', 'lsoa_code', 'lsoa_name', 'msoa_code', 'msoa_name']) {
			expect(cols).toContain(col);
		}
	});
});

describe.skipIf(!dbExists('deprivation'))('deprivation.db', () => {
	test('schema has expected columns', () => {
		const cols = columnNames(open('deprivation'), 'imd');
		for (const col of ['lsoa_code', 'rank', 'decile', 'score']) {
			expect(cols).toContain(col);
		}
	});
});

describe.skipIf(!dbExists('census'))('census.db', () => {
	test('schema has expected columns', () => {
		const cols = columnNames(open('census'), 'tenure');
		for (const col of ['lsoa_code', 'total_households', 'social_housing_pct']) {
			expect(cols).toContain(col);
		}
	});
});

describe.skipIf(!dbExists('population'))('population.db', () => {
	test('schema has expected columns', () => {
		const cols = columnNames(open('population'), 'population');
		for (const col of ['lsoa_code', 'population']) {
			expect(cols).toContain(col);
		}
	});
});

describe.skipIf(!dbExists('income'))('income.db', () => {
	test('schema has expected columns', () => {
		const cols = columnNames(open('income'), 'income');
		for (const col of ['msoa_code', 'income_bhc', 'income_ahc']) {
			expect(cols).toContain(col);
		}
	});
});

describe.skipIf(!dbExists('naptan'))('naptan.db', () => {
	test('schema has expected columns', () => {
		const cols = columnNames(open('naptan'), 'stops');
		for (const col of ['atco_code', 'common_name', 'stop_type', 'lat', 'lng']) {
			expect(cols).toContain(col);
		}
	});
});

describe.skipIf(!dbExists('flood'))('flood.db', () => {
	test('schema has expected columns', () => {
		const cols = columnNames(open('flood'), 'flood');
		for (const col of ['postcode', 'risk', 'source']) {
			expect(cols).toContain(col);
		}
	});
});

describe.skipIf(!dbExists('crime'))('crime.db', () => {
	test('schema has expected columns', () => {
		const cols = columnNames(open('crime'), 'crime_12m');
		for (const col of ['lsoa_code', 'total', 'violent', 'burglary', 'robbery']) {
			expect(cols).toContain(col);
		}
	});
});

describe.skipIf(!dbExists('broadband'))('broadband.db', () => {
	test('schema has expected columns', () => {
		const db = open('broadband');
		for (const col of ['postcode', 'outward', 'area', 'gigabit_availability']) {
			expect(columnNames(db, 'postcodes')).toContain(col);
		}
		for (const col of ['outward', 'avg_gigabit_availability']) {
			expect(columnNames(db, 'outward_aggregates')).toContain(col);
		}
		for (const col of ['area', 'avg_gigabit_availability']) {
			expect(columnNames(db, 'area_aggregates')).toContain(col);
		}
	});
});

describe.skipIf(!dbExists('uprn'))('uprn.db', () => {
	test('schema has expected columns', () => {
		const cols = columnNames(open('uprn'), 'uprn');
		for (const col of ['uprn', 'lat', 'lng']) {
			expect(cols).toContain(col);
		}
	});
});
