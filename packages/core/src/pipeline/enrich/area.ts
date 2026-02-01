/**
 * Area metrics lookup (IMD, census, flood, income, crime)
 */

import { Database } from 'bun:sqlite';
import { isAbsolute, join } from 'node:path';
import type { Listing } from '@let/core/schema';
import { log } from '@let/core/utils/logger';

type DbKey = 'postcodes' | 'deprivation' | 'census' | 'population' | 'income' | 'flood' | 'crime' | 'uprn';

type PostcodeLookup = {
	postcode: string;
	lat: number | null;
	lng: number | null;
	lsoaCode: string | null;
	lsoaName: string | null;
	msoaCode: string | null;
	msoaName: string | null;
	countryCode: string | null;
};

export type AreaEnrichmentResult = { applied: boolean };

const DB_FILES: Record<DbKey, string> = {
	postcodes: 'postcodes.db',
	deprivation: 'deprivation.db',
	census: 'census.db',
	population: 'population.db',
	income: 'income.db',
	flood: 'flood.db',
	crime: 'crime.db',
	uprn: 'uprn.db',
};

const dbCache: Partial<Record<DbKey, Database>> = {};
const dbFailed = new Set<DbKey>();

function resolveSourcesDir(): string {
	const letHome = process.env['LET_HOME'];
	if (letHome) {
		const base = isAbsolute(letHome) ? letHome : join(process.cwd(), letHome);
		return join(base, 'sources', 'db');
	}
	return join(import.meta.dirname, '..', '..', '..', '..', '..', 'sources', 'db');
}

function resolveDbPath(key: DbKey): string {
	return join(resolveSourcesDir(), DB_FILES[key]);
}

function getDb(key: DbKey): Database | null {
	if (dbCache[key]) return dbCache[key] ?? null;
	if (dbFailed.has(key)) return null;
	try {
		const db = new Database(resolveDbPath(key), { readonly: true });
		dbCache[key] = db;
		return db;
	} catch (e) {
		dbFailed.add(key);
		log.enrich.warn('Area database unavailable', { source: key, error: String(e) });
		return null;
	}
}

export function closeAreaDbs(): void {
	for (const key of Object.keys(dbCache) as DbKey[]) {
		dbCache[key]?.close();
		delete dbCache[key];
	}
	dbFailed.clear();
}

function normalizePostcode(postcode: string): string {
	return postcode.replace(/\s+/g, '').toUpperCase();
}

export function lookupPostcode(postcode: string): PostcodeLookup | null {
	const db = getDb('postcodes');
	if (!db) return null;
	const key = normalizePostcode(postcode);
	const row = db.query('SELECT postcode, postcode_display, lat, lng, lsoa_code, lsoa_name, msoa_code, msoa_name, country_code FROM postcodes WHERE postcode = ?').get(key) as {
		postcode: string;
		postcode_display: string;
		lat: number | null;
		lng: number | null;
		lsoa_code: string | null;
		lsoa_name: string | null;
		msoa_code: string | null;
		msoa_name: string | null;
		country_code: string | null;
	} | null;
	if (!row) return null;
	return {
		postcode: row.postcode_display ?? row.postcode,
		lat: row.lat,
		lng: row.lng,
		lsoaCode: row.lsoa_code,
		lsoaName: row.lsoa_name,
		msoaCode: row.msoa_code,
		msoaName: row.msoa_name,
		countryCode: row.country_code,
	};
}

function lookupImd(lsoaCode: string): { rank: number | null; decile: number | null; score: number | null } | null {
	const db = getDb('deprivation');
	if (!db) return null;
	const row = db.query('SELECT rank, decile, score FROM imd WHERE lsoa_code = ?').get(lsoaCode) as {
		rank: number | null;
		decile: number | null;
		score: number | null;
	} | null;
	return row ?? null;
}

function lookupSocialHousing(lsoaCode: string): number | null {
	const db = getDb('census');
	if (!db) return null;
	const row = db.query('SELECT social_housing_pct FROM tenure WHERE lsoa_code = ?').get(lsoaCode) as { social_housing_pct: number | null } | null;
	return row?.social_housing_pct ?? null;
}

function lookupPopulation(lsoaCode: string): number | null {
	const db = getDb('population');
	if (!db) return null;
	const row = db.query('SELECT population FROM population WHERE lsoa_code = ?').get(lsoaCode) as { population: number | null } | null;
	return row?.population ?? null;
}

function lookupIncome(msoaCode: string): { bhc: number | null; ahc: number | null } | null {
	const db = getDb('income');
	if (!db) return null;
	const row = db.query('SELECT income_bhc, income_ahc FROM income WHERE msoa_code = ?').get(msoaCode) as { income_bhc: number | null; income_ahc: number | null } | null;
	if (!row) return null;
	return { bhc: row.income_bhc, ahc: row.income_ahc };
}

function lookupFlood(postcode: string): { level: string | null; source: string | null } | null {
	const db = getDb('flood');
	if (!db) return null;
	const key = normalizePostcode(postcode);
	const row = db.query('SELECT risk, source FROM flood WHERE postcode = ?').get(key) as { risk: string | null; source: string | null } | null;
	return row ? { level: row.risk, source: row.source } : null;
}

function lookupCrime(lsoaCode: string): {
	total: number | null;
	violent: number | null;
	burglary: number | null;
	robbery: number | null;
	monthStart: string | null;
	monthEnd: string | null;
} | null {
	const db = getDb('crime');
	if (!db) return null;
	const row = db.query('SELECT total, violent, burglary, robbery, month_start, month_end FROM crime_12m WHERE lsoa_code = ?').get(lsoaCode) as {
		total: number | null;
		violent: number | null;
		burglary: number | null;
		robbery: number | null;
		month_start: string | null;
		month_end: string | null;
	} | null;
	if (!row) return null;
	return {
		total: row.total,
		violent: row.violent,
		burglary: row.burglary,
		robbery: row.robbery,
		monthStart: row.month_start,
		monthEnd: row.month_end,
	};
}

function isSupportedCountry(countryCode: string | null | undefined): boolean {
	if (!countryCode) return true;
	return countryCode === 'E92000001' || countryCode === 'W92000004';
}

function normalizeCrimeMonth(value: string | null): string | null {
	if (!value) return null;
	if (value.includes('T')) return value;
	if (/^\d{4}-\d{2}$/.test(value)) {
		return `${value}-01T00:00:00.000Z`;
	}
	if (/^\d{4}-\d{2}-\d{2}$/.test(value)) {
		return `${value}T00:00:00.000Z`;
	}
	const parsed = new Date(value);
	return Number.isNaN(parsed.getTime()) ? null : parsed.toISOString();
}

function applyGeoCodes(area: Listing['area'], lookup: PostcodeLookup): void {
	area.lsoa.code = lookup.lsoaCode ?? null;
	area.lsoa.name = lookup.lsoaName ?? null;
	area.msoa.code = lookup.msoaCode ?? null;
	area.msoa.name = lookup.msoaName ?? null;
}

function enrichFromLsoa(area: Listing['area'], lsoaCode: string): void {
	const imd = lookupImd(lsoaCode);
	if (imd) {
		area.imd.rank = imd.rank ?? null;
		area.imd.decile = imd.decile ?? null;
		area.imd.score = imd.score ?? null;
	}

	const social = lookupSocialHousing(lsoaCode);
	if (social !== null) area.socialHousingPct = Number.isNaN(social) ? null : social;

	const population = lookupPopulation(lsoaCode);
	if (population !== null) area.population = population;

	const crime = lookupCrime(lsoaCode);
	if (crime) {
		area.crime.count12m = crime.total ?? null;
		area.crime.violent12m = crime.violent ?? null;
		area.crime.burglary12m = crime.burglary ?? null;
		area.crime.robbery12m = crime.robbery ?? null;
		area.crime.updatedAt = normalizeCrimeMonth(crime.monthEnd ?? null);
	}
}

function enrichFromMsoa(area: Listing['area'], msoaCode: string): void {
	const income = lookupIncome(msoaCode);
	if (income) {
		area.income.bhc = income.bhc ?? null;
		area.income.ahc = income.ahc ?? null;
	}
}

function enrichFloodRisk(area: Listing['area'], postcode: string): void {
	const flood = lookupFlood(postcode);
	if (flood) {
		area.floodRisk.level = flood.level ?? null;
		area.floodRisk.source = flood.source ?? null;
	}
}

function computeCrimeRate(area: Listing['area']): void {
	if (!area.population || area.crime.count12m === null || area.crime.count12m === undefined) return;
	area.crime.ratePer1k = area.population > 0 ? (area.crime.count12m / area.population) * 1000 : null;
}

export function enrichListingArea(listing: Listing): AreaEnrichmentResult {
	if (!listing.postcode) return { applied: false };

	const lookup = lookupPostcode(listing.postcode);
	if (!lookup) return { applied: false };
	if (!isSupportedCountry(lookup.countryCode)) return { applied: false };

	const area = listing.area;
	applyGeoCodes(area, lookup);

	if (lookup.lsoaCode) enrichFromLsoa(area, lookup.lsoaCode);
	if (lookup.msoaCode) enrichFromMsoa(area, lookup.msoaCode);

	enrichFloodRisk(area, listing.postcode);
	computeCrimeRate(area);

	return { applied: true };
}
