/**
 * Ops command - migrate Rightmove ID primary keys to UUIDs
 */

import { Database } from 'bun:sqlite';
import { randomUUID } from 'node:crypto';
import { existsSync, renameSync, rmSync } from 'node:fs';
import { saveListingsFile as saveListingsToDb } from '@let/core/db';
import { type Listing, ListingSchema, type ListingsFile } from '@let/core/schema';
import { log } from '@let/core/utils/logger';
import { defineCommand } from 'citty';
import { LISTINGS_DB_PATH } from '../shared.js';

type LegacyListingRow = {
	id: string;
	url: string;
	address: string;
	postcode: string;
	region: string | null;
	lat: number;
	lng: number;
	pin_type: string | null;
	google_maps_url: string;
	google_maps_street_view_url: string;
	price: number;
	price_display: string;
	bedrooms: number;
	bathrooms: number;
	property_type: string;
	description: string;
	floorplan_remote: string | null;
	floorplan_local: string | null;
	epc_remote: string | null;
	epc_local: string | null;
	map_satellite_remote: string | null;
	map_satellite_local: string | null;
	map_street_remote: string | null;
	map_street_local: string | null;
	epc_rating: string | null;
	floor_area_sqm: number | null;
	epc_lodgement_date: string | null;
	epc_address_match: number | null;
	epc_search_url: string | null;
	gigabit_availability: number | null;
	listed_date: string | null;
	available_date: string | null;
	deposit: number | null;
	agent_name: string | null;
	agent_phone: string | null;
	fetched_at: string;
	extraction_status: string;
	status: string;
	notion_page_id: string | null;
	assessed_at: string | null;
	assessed_score: number | null;
};

type LegacyImageRow = { listing_id: string; remote: string; local: string | null; position: number };
type LegacyNoteRow = { listing_id: string; note: string; position: number };
type LegacyStationRow = { listing_id: string; name: string; distance: number; unit: string; position: number };
type LegacyScoreRow = {
	listing_id: string;
	overall: number;
	confidence: number;
	affordability: number;
	location: number;
	liveability: number;
	penalty_epc: number;
	penalty_garden: number;
	penalty_pets: number;
	penalty_combined: number;
	factor_monthly_rent: number;
	factor_price_percentile: number;
	factor_floor_area_sqm: number | null;
	factor_floor_area_percentile: number | null;
	factor_epc_band: string | null;
	factor_epc_numeric: number | null;
	factor_true_monthly_cost: number;
	factor_true_cost_percentile: number;
	factor_station_miles: number | null;
	factor_station_percentile: number | null;
	factor_gigabit_pct: number | null;
	factor_region_name: string | null;
	factor_priority_score: number | null;
	factor_garden_type: string;
	factor_heating_type: string;
	factor_pet_policy: string;
	factor_property_type: string | null;
	factor_bedrooms: number;
};

type LegacyAssessmentRow = {
	listing_id: string;
	maintenance: string;
	light_and_space: string;
	photo_analysis: string;
	tradeoffs: string | null;
	neighborhood_analysis: string | null;
	recommendation: string;
	family_suitability: string;
	reasoning: string;
	score_adjustment: number | null;
};

function hasColumn(db: Database, table: string, column: string): boolean {
	const rows = db.query(`PRAGMA table_info(${table})`).all() as Array<{ name: string }>;
	return rows.some((r) => r.name === column);
}

function groupBy<T>(rows: T[], key: (row: T) => string): Map<string, T[]> {
	const map = new Map<string, T[]>();
	for (const row of rows) {
		const id = key(row);
		const existing = map.get(id);
		if (existing) existing.push(row);
		else map.set(id, [row]);
	}
	return map;
}

function mapBy<T>(rows: T[], key: (row: T) => string): Map<string, T> {
	const map = new Map<string, T>();
	for (const row of rows) {
		map.set(key(row), row);
	}
	return map;
}

const emptyScoreContext: NonNullable<Listing['scores']>['context'] = {
	configHash: 'legacy',
	percentiles: {
		prices: { min: 0, max: 0, mean: 0, median: 0, stdDev: 0 },
		trueCosts: { min: 0, max: 0, mean: 0, median: 0, stdDev: 0 },
		floorAreas: { min: 0, max: 0, mean: 0, median: 0, stdDev: 0 },
		stationDistances: { min: 0, max: 0, mean: 0, median: 0, stdDev: 0 },
		crimeRates: { min: 0, max: 0, mean: 0, median: 0, stdDev: 0 },
	},
};

function buildScores(row: LegacyScoreRow | undefined): Listing['scores'] {
	if (!row) return null;
	return {
		_overall: row.overall,
		confidence: row.confidence,
		affordability: row.affordability,
		location: row.location,
		liveability: row.liveability,
		penalties: {
			epc: row.penalty_epc,
			garden: row.penalty_garden,
			pets: row.penalty_pets,
			combined: row.penalty_combined,
		},
		factors: {
			monthlyRent: row.factor_monthly_rent,
			pricePercentile: row.factor_price_percentile,
			floorAreaSqm: row.factor_floor_area_sqm,
			floorAreaPercentile: row.factor_floor_area_percentile,
			epcBand: row.factor_epc_band,
			epcNumeric: row.factor_epc_numeric,
			trueMonthlyCost: row.factor_true_monthly_cost,
			trueCostPercentile: row.factor_true_cost_percentile,
			stationMiles: row.factor_station_miles,
			stationPercentile: row.factor_station_percentile,
			gigabitPct: row.factor_gigabit_pct,
			regionName: row.factor_region_name,
			priorityScore: row.factor_priority_score,
			imdDecile: null,
			crimeRatePer1k: null,
			crimeRatePercentile: null,
			gardenType: row.factor_garden_type as NonNullable<Listing['scores']>['factors']['gardenType'],
			heatingType: row.factor_heating_type as NonNullable<Listing['scores']>['factors']['heatingType'],
			petPolicy: row.factor_pet_policy as NonNullable<Listing['scores']>['factors']['petPolicy'],
			propertyType: row.factor_property_type,
			bedrooms: row.factor_bedrooms,
		},
		context: emptyScoreContext,
	};
}

function buildAssessment(row: LegacyAssessmentRow | undefined): Listing['assessment'] {
	if (!row) return null;
	return {
		maintenance: row.maintenance as NonNullable<Listing['assessment']>['maintenance'],
		lightAndSpace: row.light_and_space,
		photoAnalysis: row.photo_analysis,
		tradeoffs: row.tradeoffs ?? undefined,
		neighborhoodAnalysis: row.neighborhood_analysis ?? undefined,
		recommendation: row.recommendation as NonNullable<Listing['assessment']>['recommendation'],
		familySuitability: row.family_suitability as NonNullable<Listing['assessment']>['familySuitability'],
		reasoning: row.reasoning,
		scoreAdjustment: row.score_adjustment ?? undefined,
	};
}

function buildListings(db: Database): Listing[] {
	const listings = db.query('SELECT * FROM listings').all() as LegacyListingRow[];
	const images = db.query('SELECT * FROM images ORDER BY listing_id, position').all() as LegacyImageRow[];
	const notes = db.query('SELECT * FROM notes ORDER BY listing_id, position').all() as LegacyNoteRow[];
	const stations = db.query('SELECT * FROM stations ORDER BY listing_id, position').all() as LegacyStationRow[];
	const scores = db.query('SELECT * FROM scores').all() as LegacyScoreRow[];
	const assessments = db.query('SELECT * FROM assessments').all() as LegacyAssessmentRow[];

	const imagesByListing = groupBy(images, (row) => row.listing_id);
	const notesByListing = groupBy(notes, (row) => row.listing_id);
	const stationsByListing = groupBy(stations, (row) => row.listing_id);
	const scoresByListing = mapBy(scores, (row) => row.listing_id);
	const assessmentsByListing = mapBy(assessments, (row) => row.listing_id);

	const output: Listing[] = [];
	for (const row of listings) {
		const listingId = randomUUID();
		const listing: Listing = ListingSchema.parse({
			id: listingId,
			portalIds: { rightmove: row.id },
			uprn: null,
			uprnSource: null,
			uprnConfidence: null,
			url: row.url,
			location: { lat: row.lat, lng: row.lng, pinType: row.pin_type },
			postcode: row.postcode,
			address: row.address,
			region: row.region,
			googleMapsUrl: row.google_maps_url,
			googleMapsStreetViewUrl: row.google_maps_street_view_url,
			area: {
				lsoa: { code: null, name: null },
				msoa: { code: null, name: null },
				imd: { rank: null, decile: null, score: null },
				income: { bhc: null, ahc: null },
				socialHousingPct: null,
				population: null,
				floodRisk: { level: null, source: null },
				crime: {
					count12m: null,
					ratePer1k: null,
					violent12m: null,
					burglary12m: null,
					robbery12m: null,
					band: null,
					trend: null,
					updatedAt: null,
				},
			},
			price: row.price,
			priceDisplay: row.price_display,
			bedrooms: row.bedrooms,
			bathrooms: row.bathrooms,
			propertyType: row.property_type,
			description: row.description,
			notes: notesByListing.get(row.id)?.map((note) => note.note) ?? [],
			images: imagesByListing.get(row.id)?.map((image) => ({ remote: image.remote, local: image.local })) ?? [],
			floorplan: { remote: row.floorplan_remote, local: row.floorplan_local },
			epc: { remote: row.epc_remote, local: row.epc_local },
			mapViews: {
				satellite: { remote: row.map_satellite_remote, local: row.map_satellite_local },
				street: { remote: row.map_street_remote, local: row.map_street_local },
			},
			epcRating: row.epc_rating,
			floorAreaSqm: row.floor_area_sqm,
			epcLodgementDate: row.epc_lodgement_date,
			epcAddressMatch: row.epc_address_match === null ? null : row.epc_address_match === 1,
			epcSearchUrl: row.epc_search_url,
			nearestStations:
				stationsByListing.get(row.id)?.map((station) => ({
					name: station.name,
					distance: station.distance,
					unit: station.unit,
				})) ?? [],
			gigabitAvailability: row.gigabit_availability,
			listedDate: row.listed_date,
			lettings: { availableDate: row.available_date, deposit: row.deposit },
			agent: { name: row.agent_name, phone: row.agent_phone },
			assessment: buildAssessment(assessmentsByListing.get(row.id)),
			assessedAt: row.assessed_at,
			assessedScore: row.assessed_score,
			scores: buildScores(scoresByListing.get(row.id)),
			fetchedAt: row.fetched_at,
			extractionStatus: row.extraction_status as Listing['extractionStatus'],
			status: row.status as Listing['status'],
			notionPageId: row.notion_page_id ?? undefined,
		});

		output.push(listing);
	}

	return output;
}

async function migrateDatabase(): Promise<void> {
	if (!existsSync(LISTINGS_DB_PATH)) {
		log.cli.error('No database found to migrate', { path: LISTINGS_DB_PATH });
		return;
	}

	const legacyDb = new Database(LISTINGS_DB_PATH, { readonly: true });
	let closed = false;
	try {
		if (hasColumn(legacyDb, 'listings', 'portal_rightmove')) {
			log.cli.info('Database already migrated');
			return;
		}

		const meta = legacyDb.query('SELECT updated_at, last_search_total FROM meta WHERE id = 1').get() as {
			updated_at: string;
			last_search_total: number;
		} | null;
		const searchUrls = legacyDb.query('SELECT url FROM search_urls ORDER BY url').all() as Array<{ url: string }>;
		const locations = legacyDb.query('SELECT name FROM search_locations ORDER BY name').all() as Array<{ name: string }>;

		const listings = buildListings(legacyDb);

		const output: ListingsFile = {
			updatedAt: meta?.updated_at ?? new Date(0).toISOString(),
			searchUrls: searchUrls.map((row) => row.url),
			locations: locations.map((row) => row.name),
			lastSearchTotal: meta?.last_search_total ?? 0,
			listings,
		};

		const tempPath = `${LISTINGS_DB_PATH}.migrated`;
		if (existsSync(tempPath)) rmSync(tempPath);
		saveListingsToDb(tempPath, output);

		legacyDb.close();
		closed = true;

		const legacyPath = `${LISTINGS_DB_PATH}.legacy`;
		if (existsSync(legacyPath)) rmSync(legacyPath);
		renameSync(LISTINGS_DB_PATH, legacyPath);
		renameSync(tempPath, LISTINGS_DB_PATH);

		log.cli.success('Migration complete', { listings: listings.length, path: LISTINGS_DB_PATH, legacy: legacyPath });
	} finally {
		if (!closed) legacyDb.close();
	}
}

export const migrateCommand = defineCommand({
	meta: {
		name: 'migrate',
		description: 'Migrate Rightmove IDs to UUID primary keys (one-time)',
	},
	async run() {
		await migrateDatabase();
	},
});
