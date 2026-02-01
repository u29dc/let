/**
 * View module - Pure functions for filtering, sorting, and statistics
 *
 * Used by `let view` commands to display and analyze listings.
 * All functions are pure and testable.
 */

import type { Listing } from '../../schema/index.js';
import { extractRegionName } from '../score/factors/extract.js';
import { normalizePropertyType } from '../score/math/utilities.js';

// =============================================================================
// TYPES
// =============================================================================

/** Filter options for listing queries */
export interface ViewerFilters {
	/** Limit to top N results (after sorting) */
	top?: number | undefined;
	/** Minimum score threshold (0-100) */
	minScore?: number | undefined;
	/** Filter by region name (case-insensitive partial match) */
	region?: string | undefined;
	/** Filter by property type (comma-separated, e.g., "flat,terraced") */
	type?: string | undefined;
}

/** Available sort fields */
export type SortField = 'score' | 'price' | 'bedrooms' | 'date';

/** Sort fields for region comparison */
export type RegionSortField = 'score' | 'price' | 'count' | 'area' | 'station' | 'gigabit' | 'garden' | 'gas' | 'top';

/** Aggregated statistics for a single region */
export interface RegionStats {
	region: string;
	count: number;
	avgScore: number;
	avgPrice: number;
	medianPrice: number;
	minPrice: number;
	maxPrice: number;
	epcTrend: string; // e.g., "C/D" or "B/C"
	avgArea: number | null; // sqm, null if insufficient data
	avgStation: number | null; // miles
	gigabitPct: number;
	gardenPct: number;
	gasPct: number;
	topPct: number; // % scoring 85+
}

/** Aggregate statistics for listings */
export interface ListingStats {
	/** Total number of listings */
	total: number;
	/** Breakdown by region */
	byRegion: Array<{ region: string; count: number; percent: number }>;
	/** Breakdown by bedroom count */
	byBedrooms: Array<{ bedrooms: number; count: number; percent: number }>;
	/** Score distribution */
	scoreDistribution: Array<{ label: string; min: number; max: number; count: number; percent: number }>;
	/** Price statistics */
	price: { min: number; max: number; avg: number; median: number };
	/** Score statistics */
	score: { min: number; max: number; avg: number; median: number };
}

/** Row data for table display */
export interface TableRow {
	id: string;
	address: string;
	price: number;
	priceDisplay: string;
	bedrooms: number;
	score: number | null;
	assessedScore: number | null;
	scoreChange: number | null;
	station: string;
	region: string;
	url: string;
}

// =============================================================================
// FILTER FUNCTIONS
// =============================================================================

/**
 * Get region name for a listing - use stored region or fall back to address extraction
 */
function getListingRegion(listing: Listing): string | null {
	return extractRegionName(listing);
}

/**
 * Filter listings by region name (case-insensitive partial match)
 */
export function filterByRegion(listings: Listing[], region: string): Listing[] {
	const normalized = region.toLowerCase();
	return listings.filter((l) => {
		const listingRegion = getListingRegion(l);
		if (!listingRegion) return false;
		return listingRegion.toLowerCase().includes(normalized);
	});
}

/**
 * Filter listings by minimum score threshold
 */
export function filterByMinScore(listings: Listing[], minScore: number): Listing[] {
	return listings.filter((l) => (l.scores?._overall ?? 0) >= minScore);
}

/**
 * Filter listings by property type (comma-separated, case-insensitive)
 * Matches against both normalized and raw propertyType for flexibility
 */
export function filterByType(listings: Listing[], types: string): Listing[] {
	const typeList = types.split(',').map((t) => t.trim().toLowerCase());
	return listings.filter((listing) => {
		if (!listing.propertyType) return false;
		const normalized = normalizePropertyType(listing.propertyType) ?? '';
		const raw = listing.propertyType.toLowerCase();
		return typeList.some((t) => normalized.includes(t) || raw.includes(t));
	});
}

/**
 * Apply all filters to listings
 */
export function filterListings(listings: Listing[], filters: ViewerFilters): Listing[] {
	let result = [...listings];

	if (filters.region) {
		result = filterByRegion(result, filters.region);
	}

	if (filters.type) {
		result = filterByType(result, filters.type);
	}

	if (filters.minScore !== undefined && filters.minScore > 0) {
		result = filterByMinScore(result, filters.minScore);
	}

	return result;
}

// =============================================================================
// SORT FUNCTIONS
// =============================================================================

/**
 * Get the sort value for a listing by field
 */
function getSortValue(listing: Listing, field: SortField): number {
	switch (field) {
		case 'score':
			// Use assessed score if available, otherwise algorithm score
			return listing.assessedScore ?? listing.scores?._overall ?? 0;
		case 'price':
			return listing.price;
		case 'bedrooms':
			return listing.bedrooms;
		case 'date':
			// Parse listedDate (YYYY-MM-DD) to timestamp for sorting
			return listing.listedDate ? new Date(listing.listedDate).getTime() : 0;
	}
}

/**
 * Sort listings by field
 * @param desc - true for descending (default), false for ascending
 */
export function sortListings(listings: Listing[], field: SortField, desc = true): Listing[] {
	const sorted = [...listings].sort((a, b) => {
		const aVal = getSortValue(a, field);
		const bVal = getSortValue(b, field);
		return desc ? bVal - aVal : aVal - bVal;
	});
	return sorted;
}

/**
 * Apply filters, sort, and limit to get final results
 */
export function queryListings(listings: Listing[], filters: ViewerFilters, sortField: SortField = 'score', desc = true): Listing[] {
	let result = filterListings(listings, filters);
	result = sortListings(result, sortField, desc);

	if (filters.top !== undefined && filters.top > 0) {
		result = result.slice(0, filters.top);
	}

	return result;
}

// =============================================================================
// STATISTICS FUNCTIONS
// =============================================================================

/**
 * Compute median of a sorted numeric array
 */
function median(sorted: number[]): number {
	if (sorted.length === 0) return 0;
	const mid = Math.floor(sorted.length / 2);
	return sorted.length % 2 !== 0 ? (sorted[mid] ?? 0) : ((sorted[mid - 1] ?? 0) + (sorted[mid] ?? 0)) / 2;
}

/**
 * Compute average of a numeric array
 */
function average(arr: number[]): number {
	if (arr.length === 0) return 0;
	return arr.reduce((sum, v) => sum + v, 0) / arr.length;
}

/**
 * Compute aggregate statistics for listings
 */
export function computeStats(listings: Listing[]): ListingStats {
	const total = listings.length;
	if (total === 0) {
		return {
			total: 0,
			byRegion: [],
			byBedrooms: [],
			scoreDistribution: [],
			price: { min: 0, max: 0, avg: 0, median: 0 },
			score: { min: 0, max: 0, avg: 0, median: 0 },
		};
	}

	// By region
	const regionCountMap = new Map<string, number>();
	for (const l of listings) {
		const region = getListingRegion(l) ?? 'Unknown';
		regionCountMap.set(region, (regionCountMap.get(region) ?? 0) + 1);
	}
	const byRegion = Array.from(regionCountMap.entries())
		.map(([region, count]) => ({ region, count, percent: Math.round((count / total) * 1000) / 10 }))
		.sort((a, b) => b.count - a.count);

	// By bedrooms
	const bedroomMap = new Map<number, number>();
	for (const l of listings) {
		bedroomMap.set(l.bedrooms, (bedroomMap.get(l.bedrooms) ?? 0) + 1);
	}
	const byBedrooms = Array.from(bedroomMap.entries())
		.map(([bedrooms, count]) => ({ bedrooms, count, percent: Math.round((count / total) * 1000) / 10 }))
		.sort((a, b) => a.bedrooms - b.bedrooms);

	// Score distribution
	const scoreRanges = [
		{ label: 'Excellent', min: 80, max: 100 },
		{ label: 'Good', min: 60, max: 79 },
		{ label: 'Average', min: 40, max: 59 },
		{ label: 'Below Avg', min: 20, max: 39 },
		{ label: 'Poor', min: 0, max: 19 },
	];
	const scoreDistribution = scoreRanges.map(({ label, min, max }) => {
		const count = listings.filter((l) => {
			const score = l.scores?._overall ?? 0;
			return score >= min && score <= max;
		}).length;
		return { label, min, max, count, percent: Math.round((count / total) * 1000) / 10 };
	});

	// Price stats
	const prices = listings.map((l) => l.price).sort((a, b) => a - b);
	const priceStats = {
		min: prices[0] ?? 0,
		max: prices[prices.length - 1] ?? 0,
		avg: Math.round(average(prices)),
		median: Math.round(median(prices)),
	};

	// Score stats
	const scores = listings
		.map((l) => l.scores?._overall ?? 0)
		.filter((s) => s > 0)
		.sort((a, b) => a - b);
	const scoreStats = {
		min: scores[0] ?? 0,
		max: scores[scores.length - 1] ?? 0,
		avg: Math.round(average(scores)),
		median: Math.round(median(scores)),
	};

	return {
		total,
		byRegion,
		byBedrooms,
		scoreDistribution,
		price: priceStats,
		score: scoreStats,
	};
}

/** Get dominant EPC ratings (e.g., "C/D") */
function getEpcTrend(listings: Listing[]): string {
	const counts: Record<string, number> = {};
	for (const l of listings) {
		if (l.epcRating) counts[l.epcRating] = (counts[l.epcRating] ?? 0) + 1;
	}
	const sorted = Object.entries(counts)
		.sort((a, b) => b[1] - a[1])
		.slice(0, 2)
		.map(([rating]) => rating);
	return sorted.join('/') || '-';
}

/** Compute percentage of listings matching a predicate */
function pct(listings: Listing[], predicate: (l: Listing) => boolean): number {
	return Math.round((listings.filter(predicate).length / listings.length) * 100);
}

/** Compute stats for a single region */
function computeSingleRegionStats(region: string, listings: Listing[]): RegionStats {
	const prices = listings.map((l) => l.price).sort((a, b) => a - b);
	const scores = listings.map((l) => l.scores?._overall ?? 0);
	const areas = listings.map((l) => l.floorAreaSqm).filter((a): a is number => a !== null);
	const stations = listings.map((l) => l.nearestStations[0]?.distance).filter((d): d is number => d !== undefined);

	return {
		region,
		count: listings.length,
		avgScore: Math.round(average(scores)),
		avgPrice: Math.round(average(prices)),
		medianPrice: Math.round(median(prices)),
		minPrice: prices[0] ?? 0,
		maxPrice: prices[prices.length - 1] ?? 0,
		epcTrend: getEpcTrend(listings),
		avgArea: areas.length > 0 ? Math.round(average(areas)) : null,
		avgStation: stations.length > 0 ? Math.round(average(stations) * 10) / 10 : null,
		gigabitPct: pct(listings, (l) => l.gigabitAvailability !== null && l.gigabitAvailability >= 90),
		gardenPct: pct(listings, (l) => l.scores?.factors?.gardenType === 'private'),
		gasPct: pct(listings, (l) => l.scores?.factors?.heatingType === 'gas'),
		topPct: pct(listings, (l) => (l.scores?._overall ?? 0) >= 85),
	};
}

/**
 * Compute aggregated statistics per region
 */
export function computeRegionStats(listings: Listing[]): RegionStats[] {
	// Group listings by region
	const regionMap = new Map<string, Listing[]>();
	for (const l of listings) {
		const region = getListingRegion(l) ?? 'Unknown';
		if (!regionMap.has(region)) regionMap.set(region, []);
		regionMap.get(region)?.push(l);
	}

	// Compute stats per region
	return Array.from(regionMap.entries()).map(([region, regionListings]) => computeSingleRegionStats(region, regionListings));
}

/**
 * Sort region stats by field
 */
export function sortRegionStats(stats: RegionStats[], field: RegionSortField, desc = true): RegionStats[] {
	const getValue = (s: RegionStats): number => {
		switch (field) {
			case 'score':
				return s.avgScore;
			case 'price':
				return s.avgPrice;
			case 'count':
				return s.count;
			case 'area':
				return s.avgArea ?? 0;
			case 'station':
				return s.avgStation ?? 999; // Push nulls to end when sorting asc
			case 'gigabit':
				return s.gigabitPct;
			case 'garden':
				return s.gardenPct;
			case 'gas':
				return s.gasPct;
			case 'top':
				return s.topPct;
		}
	};
	return [...stats].sort((a, b) => (desc ? getValue(b) - getValue(a) : getValue(a) - getValue(b)));
}

// =============================================================================
// FORMATTING FUNCTIONS
// =============================================================================

/**
 * Truncate string to max length with ellipsis
 */
export function truncate(str: string, maxLen: number): string {
	if (str.length <= maxLen) return str;
	return `${str.slice(0, maxLen - 3)}...`;
}

/**
 * Format nearest station for display
 */
export function formatStation(listing: Listing): string {
	const station = listing.nearestStations[0];
	if (!station) return '--';
	const dist = station.distance.toFixed(1);
	const name = truncate(station.name, 25);
	const miles = `(${dist}mi)`;
	return `${name.padEnd(25)} ${miles}`;
}

/**
 * Format a listing as a table row
 */
export function formatTableRow(listing: Listing): TableRow {
	const displayId = listing.portalIds.rightmove ?? listing.id;
	return {
		id: displayId,
		address: truncate(listing.address, 45),
		price: listing.price,
		priceDisplay: listing.priceDisplay,
		bedrooms: listing.bedrooms,
		score: listing.scores?._overall ?? null,
		assessedScore: listing.assessedScore ?? null,
		scoreChange: listing.assessment?.scoreAdjustment ?? null,
		station: formatStation(listing),
		region: getListingRegion(listing) ?? 'Unknown',
		url: listing.url.replace('www.', ''),
	};
}

// =============================================================================
// INDEXED LOOKUP
// =============================================================================

/** Module-level cache for O(1) ID lookups */
type ListingIndex = { byUuid: Map<string, Listing>; byRightmove: Map<string, Listing> };
let listingsIndex: ListingIndex | null = null;
let indexedListingsRef: Listing[] | null = null;

function isUuid(value: string): boolean {
	return /^[0-9a-f]{8}-[0-9a-f]{4}-[1-5][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i.test(value);
}

/**
 * Build or retrieve cached index for O(1) lookups
 * Cache is invalidated when a different listings array is passed
 */
function ensureIndex(listings: Listing[]): ListingIndex {
	if (listingsIndex && indexedListingsRef === listings) {
		return listingsIndex;
	}
	const byUuid = new Map<string, Listing>();
	const byRightmove = new Map<string, Listing>();
	for (const listing of listings) {
		byUuid.set(listing.id, listing);
		const rightmoveId = listing.portalIds.rightmove;
		if (rightmoveId) byRightmove.set(rightmoveId, listing);
	}
	listingsIndex = { byUuid, byRightmove };
	indexedListingsRef = listings;
	return listingsIndex;
}

/**
 * Find a listing by ID using O(1) Map lookup
 */
export function findListingById(listings: Listing[], id: string): Listing | undefined {
	const index = ensureIndex(listings);
	if (isUuid(id)) return index.byUuid.get(id);
	return index.byRightmove.get(id) ?? index.byUuid.get(id);
}
