/**
 * SQLite persistence for listings data
 *
 * Fully normalized schema with foreign keys.
 */

import { Database } from 'bun:sqlite';
import { copyFileSync, existsSync } from 'node:fs';
import { type Listing, ListingSchema, type ListingsFile, ListingsFileSchema } from '../schema/index.js';
import schemaSQL from './schema.sql' with { type: 'text' };

type DbMeta = { updatedAt: string; lastSearchTotal: number };

type ListingRow = {
	id: string;
	portal_rightmove: string | null;
	portal_zoopla: string | null;
	portal_onthemarket: string | null;
	uprn: string | null;
	uprn_source: string | null;
	uprn_confidence: string | null;
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
	area_lsoa_code: string | null;
	area_lsoa_name: string | null;
	area_msoa_code: string | null;
	area_msoa_name: string | null;
	imd_rank: number | null;
	imd_decile: number | null;
	imd_score: number | null;
	income_bhc: number | null;
	income_ahc: number | null;
	social_housing_pct: number | null;
	population: number | null;
	flood_risk_level: string | null;
	flood_risk_source: string | null;
	crime_count_12m: number | null;
	crime_rate_per_1k: number | null;
	crime_violent_12m: number | null;
	crime_burglary_12m: number | null;
	crime_robbery_12m: number | null;
	crime_band: string | null;
	crime_trend: string | null;
	crime_updated_at: string | null;
	fetched_at: string;
	extraction_status: string;
	status: string;
	notion_page_id: string | null;
	assessed_at: string | null;
	assessed_score: number | null;
};

type ImageRow = { listing_id: string; remote: string; local: string | null; position: number };
type NoteRow = { listing_id: string; note: string; position: number };
type StationRow = { listing_id: string; name: string; distance: number; unit: string; position: number };
type ScoreRow = {
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
	factor_imd_decile: number | null;
	factor_crime_rate_per_1k: number | null;
	factor_crime_rate_percentile: number | null;
	factor_garden_type: string;
	factor_heating_type: string;
	factor_pet_policy: string;
	factor_property_type: string | null;
	factor_bedrooms: number;
};

type AssessmentRow = {
	listing_id: string;
	maintenance: string;
	light_and_space: string;
	photo_analysis: string;
	tradeoffs: string | null;
	neighborhood_analysis: string | null;
	recommendation: string;
	family_suitability: string;
	reasoning: string;
	score_adjustment: number;
};

type ListingScores = NonNullable<Listing['scores']>;
type ListingAssessment = NonNullable<Listing['assessment']>;
type ListingValue = string | number | null;

export function openListingsDb(path: string): Database {
	const db = new Database(path);
	db.run('PRAGMA foreign_keys = ON');
	initSchema(db);
	return db;
}

export function closeListingsDb(db: Database): void {
	db.close();
}

function initSchema(db: Database): void {
	db.exec(schemaSQL);
}

function loadMeta(db: Database): DbMeta {
	const row = db.query('SELECT updated_at, last_search_total FROM meta WHERE id = 1').get() as { updated_at: string; last_search_total: number } | undefined;
	if (!row) {
		return { updatedAt: new Date(0).toISOString(), lastSearchTotal: 0 };
	}
	return { updatedAt: row.updated_at, lastSearchTotal: row.last_search_total };
}

export function loadListingsFile(dbPath: string): ListingsFile {
	const db = openListingsDb(dbPath);
	try {
		const meta = loadMeta(db);
		const searchUrls = db.query('SELECT url FROM search_urls ORDER BY url').all() as Array<{ url: string }>;
		const locations = db.query('SELECT name FROM search_locations ORDER BY name').all() as Array<{ name: string }>;
		const listings = hydrateListings(db);

		const data: ListingsFile = {
			updatedAt: meta.updatedAt,
			searchUrls: searchUrls.map((row) => row.url),
			locations: locations.map((row) => row.name),
			lastSearchTotal: meta.lastSearchTotal,
			listings,
		};

		return ListingsFileSchema.parse(data);
	} finally {
		closeListingsDb(db);
	}
}

const DEFAULT_MAP_VIEWS: NonNullable<Listing['mapViews']> = {
	satellite: { remote: null, local: null },
	street: { remote: null, local: null },
};

type Statement = ReturnType<Database['prepare']>;
type InsertStatements = {
	listing: Statement;
	station: Statement;
	image: Statement;
	note: Statement;
	score: Statement;
	assessment: Statement;
	searchUrl: Statement;
	location: Statement;
};

function clearListingsTables(db: Database): void {
	db.run('DELETE FROM images');
	db.run('DELETE FROM notes');
	db.run('DELETE FROM stations');
	db.run('DELETE FROM scores');
	db.run('DELETE FROM assessments');
	db.run('DELETE FROM listings');
	db.run('DELETE FROM search_urls');
	db.run('DELETE FROM search_locations');
	db.run('DELETE FROM meta');
}

function createInsertStatements(db: Database): InsertStatements {
	return {
		listing: db.prepare(`
			INSERT INTO listings (
				id, portal_rightmove, portal_zoopla, portal_onthemarket,
				uprn, uprn_source, uprn_confidence,
				url, address, postcode, region, lat, lng, pin_type,
				google_maps_url, google_maps_street_view_url,
				price, price_display, bedrooms, bathrooms, property_type,
				description, floorplan_remote, floorplan_local, epc_remote, epc_local,
				map_satellite_remote, map_satellite_local, map_street_remote, map_street_local,
				epc_rating, floor_area_sqm, epc_lodgement_date, epc_address_match, epc_search_url,
				gigabit_availability, listed_date, available_date, deposit,
				agent_name, agent_phone,
				area_lsoa_code, area_lsoa_name, area_msoa_code, area_msoa_name,
				imd_rank, imd_decile, imd_score, income_bhc, income_ahc,
				social_housing_pct, population, flood_risk_level, flood_risk_source,
				crime_count_12m, crime_rate_per_1k, crime_violent_12m, crime_burglary_12m, crime_robbery_12m,
				crime_band, crime_trend, crime_updated_at,
				fetched_at, extraction_status, status, notion_page_id,
				assessed_at, assessed_score
			) VALUES (
				?, ?, ?, ?,
				?, ?, ?,
				?, ?, ?, ?, ?, ?, ?,
				?, ?,
				?, ?, ?, ?, ?,
				?, ?, ?, ?, ?,
				?, ?, ?, ?,
				?, ?, ?, ?, ?,
				?, ?, ?, ?,
				?, ?,
				?, ?, ?, ?,
				?, ?, ?, ?, ?,
				?, ?, ?, ?,
				?, ?, ?, ?, ?,
				?, ?, ?,
				?, ?, ?, ?,
				?, ?
			);
		`),
		station: db.prepare('INSERT INTO stations (listing_id, name, distance, unit, position) VALUES (?, ?, ?, ?, ?);'),
		image: db.prepare('INSERT INTO images (listing_id, remote, local, position) VALUES (?, ?, ?, ?);'),
		note: db.prepare('INSERT INTO notes (listing_id, note, position) VALUES (?, ?, ?);'),
		score: db.prepare(`
			INSERT INTO scores (
				listing_id, overall, confidence, affordability, location, liveability,
				penalty_epc, penalty_garden, penalty_pets, penalty_combined,
				factor_monthly_rent, factor_price_percentile, factor_floor_area_sqm, factor_floor_area_percentile,
				factor_epc_band, factor_epc_numeric, factor_true_monthly_cost, factor_true_cost_percentile,
				factor_station_miles, factor_station_percentile, factor_gigabit_pct, factor_region_name,
				factor_priority_score, factor_imd_decile, factor_crime_rate_per_1k, factor_crime_rate_percentile,
				factor_garden_type, factor_heating_type, factor_pet_policy, factor_property_type, factor_bedrooms
			) VALUES (
				?, ?, ?, ?, ?, ?,
				?, ?, ?, ?,
				?, ?, ?, ?,
				?, ?, ?, ?,
				?, ?, ?, ?,
				?, ?, ?, ?,
				?, ?, ?, ?, ?
			);
		`),
		assessment: db.prepare(`
			INSERT INTO assessments (
				listing_id, maintenance, light_and_space, photo_analysis, tradeoffs,
				neighborhood_analysis, recommendation, family_suitability, reasoning, score_adjustment
			) VALUES (
				?, ?, ?, ?, ?,
				?, ?, ?, ?, ?
			);
		`),
		searchUrl: db.prepare('INSERT INTO search_urls (url) VALUES (?);'),
		location: db.prepare('INSERT INTO search_locations (name) VALUES (?);'),
	};
}

function insertMetaRow(db: Database, meta: DbMeta): void {
	db.run('INSERT INTO meta (id, updated_at, last_search_total) VALUES (1, ?, ?);', [meta.updatedAt, meta.lastSearchTotal]);
}

function insertSearchUrls(statements: InsertStatements, urls: string[]): void {
	for (const url of urls) {
		statements.searchUrl.run(url);
	}
}

function insertLocations(statements: InsertStatements, locations: string[]): void {
	for (const name of locations) {
		statements.location.run(name);
	}
}

function normalizeEpcAddressMatch(value: boolean | null | undefined): number | null {
	if (value === null || value === undefined) return null;
	return value ? 1 : 0;
}

function resolveMapViews(listing: Listing): NonNullable<Listing['mapViews']> {
	return listing.mapViews ?? DEFAULT_MAP_VIEWS;
}

function buildListingIdentityValues(listing: Listing): ListingValue[] {
	return [
		listing.id,
		listing.portalIds.rightmove ?? null,
		listing.portalIds.zoopla ?? null,
		listing.portalIds.onthemarket ?? null,
		listing.uprn ?? null,
		listing.uprnSource ?? null,
		listing.uprnConfidence ?? null,
		listing.url,
		listing.address,
		listing.postcode,
		listing.region ?? null,
	];
}

function buildListingLocationValues(listing: Listing): ListingValue[] {
	return [listing.location.lat, listing.location.lng, listing.location.pinType ?? null, listing.googleMapsUrl, listing.googleMapsStreetViewUrl];
}

function buildListingPropertyValues(listing: Listing): ListingValue[] {
	return [listing.price, listing.priceDisplay, listing.bedrooms, listing.bathrooms, listing.propertyType];
}

function buildListingContentValues(listing: Listing): ListingValue[] {
	return [listing.description, listing.floorplan.remote ?? null, listing.floorplan.local ?? null, listing.epc.remote ?? null, listing.epc.local ?? null];
}

function buildListingMapValues(listing: Listing): ListingValue[] {
	const mapViews = resolveMapViews(listing);
	return [mapViews.satellite.remote ?? null, mapViews.satellite.local ?? null, mapViews.street.remote ?? null, mapViews.street.local ?? null];
}

function buildListingEpcValues(listing: Listing): ListingValue[] {
	return [listing.epcRating ?? null, listing.floorAreaSqm ?? null, listing.epcLodgementDate ?? null, normalizeEpcAddressMatch(listing.epcAddressMatch), listing.epcSearchUrl ?? null];
}

function buildListingAvailabilityValues(listing: Listing): ListingValue[] {
	return [listing.gigabitAvailability ?? null, listing.listedDate ?? null, listing.lettings.availableDate ?? null, listing.lettings.deposit ?? null];
}

function buildListingAgentValues(listing: Listing): ListingValue[] {
	return [listing.agent.name ?? null, listing.agent.phone ?? null];
}

function buildAreaCensusValues(area: Listing['area']): ListingValue[] {
	return [
		area.lsoa.code ?? null,
		area.lsoa.name ?? null,
		area.msoa.code ?? null,
		area.msoa.name ?? null,
		area.imd.rank ?? null,
		area.imd.decile ?? null,
		area.imd.score ?? null,
		area.income.bhc ?? null,
		area.income.ahc ?? null,
		area.socialHousingPct ?? null,
		area.population ?? null,
	];
}

function buildAreaRiskValues(area: Listing['area']): ListingValue[] {
	return [
		area.floodRisk.level ?? null,
		area.floodRisk.source ?? null,
		area.crime.count12m ?? null,
		area.crime.ratePer1k ?? null,
		area.crime.violent12m ?? null,
		area.crime.burglary12m ?? null,
		area.crime.robbery12m ?? null,
		area.crime.band ?? null,
		area.crime.trend ?? null,
		area.crime.updatedAt ?? null,
	];
}

function buildListingAreaValues(listing: Listing): ListingValue[] {
	const area = listing.area;
	return [...buildAreaCensusValues(area), ...buildAreaRiskValues(area)];
}

function buildListingMetaValues(listing: Listing): ListingValue[] {
	return [listing.fetchedAt, listing.extractionStatus, listing.status, listing.notionPageId ?? null, listing.assessedAt ?? null, listing.assessedScore ?? null];
}

function buildListingValues(listing: Listing): ListingValue[] {
	return [
		...buildListingIdentityValues(listing),
		...buildListingLocationValues(listing),
		...buildListingPropertyValues(listing),
		...buildListingContentValues(listing),
		...buildListingMapValues(listing),
		...buildListingEpcValues(listing),
		...buildListingAvailabilityValues(listing),
		...buildListingAgentValues(listing),
		...buildListingAreaValues(listing),
		...buildListingMetaValues(listing),
	];
}

function insertListingRow(statements: InsertStatements, listing: Listing): void {
	const values = buildListingValues(listing);
	statements.listing.run(...values);
}

function insertStations(statements: InsertStatements, listing: Listing): void {
	for (const [index, station] of listing.nearestStations.entries()) {
		statements.station.run(listing.id, station.name, station.distance, station.unit, index);
	}
}

function insertImages(statements: InsertStatements, listing: Listing): void {
	for (const [index, image] of listing.images.entries()) {
		statements.image.run(listing.id, image.remote, image.local ?? null, index);
	}
}

function insertNotes(statements: InsertStatements, listing: Listing): void {
	for (const [index, note] of listing.notes.entries()) {
		statements.note.run(listing.id, note, index);
	}
}

function insertScores(statements: InsertStatements, listing: Listing): void {
	const scores = listing.scores;
	if (!scores) return;
	const values: ListingValue[] = [
		listing.id,
		scores._overall,
		scores.confidence,
		scores.affordability,
		scores.location,
		scores.liveability,
		scores.penalties.epc,
		scores.penalties.garden,
		scores.penalties.pets,
		scores.penalties.combined,
		scores.factors.monthlyRent,
		scores.factors.pricePercentile,
		scores.factors.floorAreaSqm ?? null,
		scores.factors.floorAreaPercentile ?? null,
		scores.factors.epcBand ?? null,
		scores.factors.epcNumeric ?? null,
		scores.factors.trueMonthlyCost,
		scores.factors.trueCostPercentile,
		scores.factors.stationMiles ?? null,
		scores.factors.stationPercentile ?? null,
		scores.factors.gigabitPct ?? null,
		scores.factors.regionName ?? null,
		scores.factors.priorityScore ?? null,
		scores.factors.imdDecile ?? null,
		scores.factors.crimeRatePer1k ?? null,
		scores.factors.crimeRatePercentile ?? null,
		scores.factors.gardenType,
		scores.factors.heatingType,
		scores.factors.petPolicy,
		scores.factors.propertyType ?? null,
		scores.factors.bedrooms,
	];

	statements.score.run(...values);
}

function insertAssessment(statements: InsertStatements, listing: Listing): void {
	const assessment = listing.assessment;
	if (!assessment) return;
	const values: ListingValue[] = [
		listing.id,
		assessment.maintenance,
		assessment.lightAndSpace,
		assessment.photoAnalysis,
		assessment.tradeoffs ?? null,
		assessment.neighborhoodAnalysis ?? null,
		assessment.recommendation,
		assessment.familySuitability,
		assessment.reasoning,
		assessment.scoreAdjustment,
	];

	statements.assessment.run(...values);
}

function persistListings(statements: InsertStatements, listings: Listing[]): void {
	for (const listing of listings) {
		insertListingRow(statements, listing);
		insertStations(statements, listing);
		insertImages(statements, listing);
		insertNotes(statements, listing);
		insertScores(statements, listing);
		insertAssessment(statements, listing);
	}
}

export function saveListingsFile(dbPath: string, data: ListingsFile): void {
	const parsed = ListingsFileSchema.parse(data);

	if (existsSync(dbPath)) {
		const backupPath = `${dbPath}.bak`;
		copyFileSync(dbPath, backupPath);
	}
	const db = openListingsDb(dbPath);
	const statements = createInsertStatements(db);

	const tx = db.transaction(() => {
		clearListingsTables(db);
		insertMetaRow(db, { updatedAt: parsed.updatedAt, lastSearchTotal: parsed.lastSearchTotal });
		insertSearchUrls(statements, parsed.searchUrls);
		insertLocations(statements, parsed.locations);
		persistListings(statements, parsed.listings);
	});

	try {
		tx();
	} finally {
		closeListingsDb(db);
	}
}

function hydrateListings(db: Database): Listing[] {
	const listings = db.query('SELECT * FROM listings').all() as ListingRow[];
	const images = db.query('SELECT * FROM images ORDER BY listing_id, position').all() as ImageRow[];
	const notes = db.query('SELECT * FROM notes ORDER BY listing_id, position').all() as NoteRow[];
	const stations = db.query('SELECT * FROM stations ORDER BY listing_id, position').all() as StationRow[];
	const scores = db.query('SELECT * FROM scores').all() as ScoreRow[];
	const assessments = db.query('SELECT * FROM assessments').all() as AssessmentRow[];

	const imagesByListing = groupBy(images, (row) => row.listing_id);
	const notesByListing = groupBy(notes, (row) => row.listing_id);
	const stationsByListing = groupBy(stations, (row) => row.listing_id);
	const scoresByListing = mapBy(scores, (row) => row.listing_id);
	const assessmentsByListing = mapBy(assessments, (row) => row.listing_id);

	const result: Listing[] = [];
	for (const row of listings) {
		const listingId = row.id;
		const listing: Listing = ListingSchema.parse({
			id: listingId,
			portalIds: {
				rightmove: row.portal_rightmove ?? undefined,
				zoopla: row.portal_zoopla ?? undefined,
				onthemarket: row.portal_onthemarket ?? undefined,
			},
			uprn: row.uprn,
			uprnSource: row.uprn_source as Listing['uprnSource'],
			uprnConfidence: row.uprn_confidence as Listing['uprnConfidence'],
			url: row.url,
			location: { lat: row.lat, lng: row.lng, pinType: row.pin_type },
			postcode: row.postcode,
			address: row.address,
			region: row.region,
			googleMapsUrl: row.google_maps_url,
			googleMapsStreetViewUrl: row.google_maps_street_view_url,
			area: {
				lsoa: { code: row.area_lsoa_code, name: row.area_lsoa_name },
				msoa: { code: row.area_msoa_code, name: row.area_msoa_name },
				imd: { rank: row.imd_rank, decile: row.imd_decile, score: row.imd_score },
				income: { bhc: row.income_bhc, ahc: row.income_ahc },
				socialHousingPct: row.social_housing_pct,
				population: row.population,
				floodRisk: { level: row.flood_risk_level, source: row.flood_risk_source },
				crime: {
					count12m: row.crime_count_12m,
					ratePer1k: row.crime_rate_per_1k,
					violent12m: row.crime_violent_12m,
					burglary12m: row.crime_burglary_12m,
					robbery12m: row.crime_robbery_12m,
					band: row.crime_band as Listing['area']['crime']['band'],
					trend: row.crime_trend as Listing['area']['crime']['trend'],
					updatedAt: row.crime_updated_at,
				},
			},
			price: row.price,
			priceDisplay: row.price_display,
			bedrooms: row.bedrooms,
			bathrooms: row.bathrooms,
			propertyType: row.property_type,
			description: row.description,
			notes: notesByListing.get(listingId)?.map((note) => note.note) ?? [],
			images: imagesByListing.get(listingId)?.map((image) => ({ remote: image.remote, local: image.local })) ?? [],
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
				stationsByListing.get(listingId)?.map((station) => ({
					name: station.name,
					distance: station.distance,
					unit: station.unit,
				})) ?? [],
			gigabitAvailability: row.gigabit_availability,
			listedDate: row.listed_date,
			lettings: { availableDate: row.available_date, deposit: row.deposit },
			agent: { name: row.agent_name, phone: row.agent_phone },
			assessment: buildAssessment(assessmentsByListing.get(listingId)),
			assessedAt: row.assessed_at,
			assessedScore: row.assessed_score,
			scores: buildScores(scoresByListing.get(listingId)),
			fetchedAt: row.fetched_at,
			extractionStatus: row.extraction_status,
			status: row.status,
			notionPageId: row.notion_page_id ?? undefined,
		});
		result.push(listing);
	}

	return result;
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

function buildScores(row: ScoreRow | undefined): Listing['scores'] {
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
			imdDecile: row.factor_imd_decile ?? null,
			crimeRatePer1k: row.factor_crime_rate_per_1k ?? null,
			crimeRatePercentile: row.factor_crime_rate_percentile ?? null,
			gardenType: row.factor_garden_type as ListingScores['factors']['gardenType'],
			heatingType: row.factor_heating_type as ListingScores['factors']['heatingType'],
			petPolicy: row.factor_pet_policy as ListingScores['factors']['petPolicy'],
			propertyType: row.factor_property_type,
			bedrooms: row.factor_bedrooms,
		},
		context: emptyScoreContext,
	};
}

function buildAssessment(row: AssessmentRow | undefined): Listing['assessment'] {
	if (!row) return null;
	return {
		maintenance: row.maintenance as ListingAssessment['maintenance'],
		lightAndSpace: row.light_and_space,
		photoAnalysis: row.photo_analysis,
		tradeoffs: row.tradeoffs ?? undefined,
		neighborhoodAnalysis: row.neighborhood_analysis ?? undefined,
		recommendation: row.recommendation as ListingAssessment['recommendation'],
		familySuitability: row.family_suitability as ListingAssessment['familySuitability'],
		reasoning: row.reasoning,
		scoreAdjustment: row.score_adjustment,
	};
}

function groupBy<T>(rows: T[], key: (row: T) => string): Map<string, T[]> {
	const map = new Map<string, T[]>();
	for (const row of rows) {
		const id = key(row);
		const existing = map.get(id);
		if (existing) {
			existing.push(row);
		} else {
			map.set(id, [row]);
		}
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
