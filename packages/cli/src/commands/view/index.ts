/**
 * View commands - Display listings and analytics
 *
 * let view list          - Table of listings (with filters/sort)
 * let view detail <id>   - Full listing details
 * let view stats         - Aggregate statistics
 * let view regions       - Compare regions by aggregated metrics
 */

import {
	computeRegionStats,
	computeStats,
	findListingById,
	formatTableRow,
	queryListings,
	type RegionSortField,
	type RegionStats,
	type SortField,
	sortRegionStats,
	truncate,
} from '@let/core/pipeline/view';
import type { Listing } from '@let/core/schema';
import { log } from '@let/core/utils/logger';
import { defineCommand } from 'citty';
import {
	colorQuality,
	colorRecommendation,
	colorStatus,
	createTable,
	formatPercent,
	formatPrice,
	formatScoreWithSignal,
	formatValue,
	type KeyValueRow,
	printKeyValues,
	printTwoColumns,
	section,
	subheader,
	wrapText,
} from '../../output/index.js';

import { loadExistingListings } from '../shared-read.js';

/** Standard key width for all detail view sections */
const KEY_WIDTH = 14;

/** Valid sort fields */
const VALID_SORT_FIELDS: SortField[] = ['score', 'price', 'bedrooms', 'date'];

/** Parse and validate sort field */
function parseSortField(value: string): SortField {
	if (VALID_SORT_FIELDS.includes(value as SortField)) {
		return value as SortField;
	}
	log.cli.warn(`Invalid sort field "${value}", using "score"`, { valid: VALID_SORT_FIELDS });
	return 'score';
}

/** Format percentile for factors */
function formatPercentile(value: number | null | undefined): string {
	if (value === null || value === undefined || Number.isNaN(value)) return '--';
	return `p${Math.round(value)}`;
}

/** Format signed adjustment values */
function formatSigned(value: number | null | undefined): string {
	if (value === null || value === undefined || Number.isNaN(value)) return '--';
	return value >= 0 ? `+${value}` : `${value}`;
}

function formatWithPercentile(value: string, percentile: number | null | undefined): string {
	if (value === '--') return value;
	const pct = formatPercentile(percentile);
	return pct === '--' ? value : `${value} (${pct})`;
}

/** Format score change as signed value or empty string */
function formatScoreChange(change: number | null): string {
	if (change === null) return '';
	return change >= 0 ? `+${change}` : `${change}`;
}

/**
 * Render listings as a table
 */
function renderTable(listings: Listing[]): void {
	const table = createTable([
		{ name: 'id', title: 'ID', alignment: 'left' },
		{ name: 'address', title: 'ADDRESS', alignment: 'left' },
		{ name: 'region', title: 'REGION', alignment: 'left' },
		{ name: 'price', title: 'PRICE', alignment: 'right' },
		{ name: 'beds', title: 'BEDS', alignment: 'right' },
		{ name: 'algo', title: 'ALGO', alignment: 'right' },
		{ name: 'assessed', title: 'ASSESSED', alignment: 'right' },
		{ name: 'chg', title: 'CHG', alignment: 'right' },
		{ name: 'station', title: 'NEAREST STATION', alignment: 'left' },
		{ name: 'url', title: 'URL', alignment: 'left' },
	]);

	for (const listing of listings) {
		const row = formatTableRow(listing);
		table.addRow({
			id: row.id,
			address: row.address,
			region: truncate(row.region, 20),
			price: formatPrice(row.price),
			beds: row.bedrooms,
			algo: formatScoreWithSignal(row.score),
			assessed: row.assessedScore !== null ? formatScoreWithSignal(row.assessedScore) : '',
			chg: formatScoreChange(row.scoreChange),
			station: row.station,
			url: row.url,
		});
	}

	table.printTable();

	// Summary line
	const avgScore = listings.length > 0 ? Math.round(listings.reduce((sum, l) => sum + (l.scores?._overall ?? 0), 0) / listings.length) : null;
	printKeyValues(
		[
			['Listings', `${listings.length}`],
			['Avg score', formatScoreWithSignal(avgScore)],
		],
		{ keyWidth: 9 },
	);
}

// =============================================================================
// DETAIL VIEW - DUAL COLUMN COMPACT LAYOUT
// =============================================================================

/** Render detail header with ID | Address */
function renderDetailHeader(l: Listing): void {
	const header = l.address.includes(l.postcode) ? l.address : `${l.address}, ${l.postcode}`;
	const displayId = l.portalIds.rightmove ?? l.id;
	section(`${displayId} | ${header}`);
}

// --- Row getter functions (return data without printing) ---

function getPropertyRows(l: Listing): KeyValueRow[] {
	return [
		['Type', formatValue(l.propertyType)],
		['Beds', formatValue(l.bedrooms)],
		['Baths', formatValue(l.bathrooms)],
		['Price', formatValue(l.priceDisplay)],
		['Deposit', l.lettings.deposit ? formatPrice(l.lettings.deposit) : '--'],
		['Available', formatValue(l.lettings.availableDate)],
		['Status', colorStatus(l.status)],
	];
}

function getDetailsRows(l: Listing): KeyValueRow[] {
	return [
		['Floor Area', l.floorAreaSqm ? `${l.floorAreaSqm} sqm` : '--'],
		['EPC', formatValue(l.epcRating)],
		['Broadband', l.gigabitAvailability !== null ? `${formatPercent(l.gigabitAvailability)} gigabit` : '--'],
		['Listed', formatValue(l.listedDate)],
		['Region', formatValue(l.region)],
		['Coordinates', `${l.location.lat.toFixed(4)}, ${l.location.lng.toFixed(4)}`],
	];
}

function getStationsRows(l: Listing): KeyValueRow[] {
	if (l.nearestStations.length === 0) return [];
	const stationsText = l.nearestStations.map((s) => `${s.name} (${s.distance.toFixed(1)}mi)`).join('\n');
	return [['Stations', stationsText]];
}

function getScoresRows(l: Listing): KeyValueRow[] {
	if (!l.scores) return [];
	const s = l.scores;
	return [
		['Overall', formatScoreWithSignal(s._overall)],
		['Assessed', formatScoreWithSignal(l.assessedScore)],
		['Confidence', formatPercent(s.confidence * 100)],
		['Adjustment', formatSigned(l.assessment?.scoreAdjustment)],
		['Affordability', formatScoreWithSignal(s.affordability)],
		['Location', formatScoreWithSignal(s.location)],
		['Liveability', formatScoreWithSignal(s.liveability)],
	];
}

function getFactorsRows(l: Listing): KeyValueRow[] {
	if (!l.scores) return [];
	const f = l.scores.factors;
	const epcScore = f.epcNumeric !== null ? `${Math.round(f.epcNumeric)}/100` : '--';
	const epcDisplay = f.epcBand ? `${f.epcBand} (${epcScore})` : epcScore;
	return [
		['Rent', formatWithPercentile(formatPrice(f.monthlyRent), f.pricePercentile)],
		['True cost', formatWithPercentile(`${formatPrice(f.trueMonthlyCost)}/mo`, f.trueCostPercentile)],
		['Floor area', formatWithPercentile(f.floorAreaSqm ? `${f.floorAreaSqm} sqm` : '--', f.floorAreaPercentile)],
		['EPC', epcDisplay],
		['Station', formatWithPercentile(f.stationMiles !== null ? `${f.stationMiles.toFixed(1)}mi` : '--', f.stationPercentile)],
		['Broadband', f.gigabitPct !== null ? formatPercent(f.gigabitPct) : '--'],
		['Garden', formatValue(f.gardenType)],
		['Heating', formatValue(f.heatingType)],
		['Pets', formatValue(f.petPolicy)],
	];
}

function getAssessmentShortRows(l: Listing): KeyValueRow[] {
	if (!l.assessment) return [];
	const a = l.assessment;
	return [
		['Maintenance', colorQuality(a.maintenance)],
		['Family', colorQuality(a.familySuitability)],
		['Recommend', colorRecommendation(a.recommendation)],
		['Assessed', l.assessedAt ? (l.assessedAt.split('T')[0] ?? '--') : '--'],
		['Adj score', formatScoreWithSignal(l.assessedScore)],
		['Adjustment', formatSigned(a.scoreAdjustment)],
	];
}

function getAssessmentLongRows(l: Listing, valueWidth: number): KeyValueRow[] {
	if (!l.assessment) return [];
	const a = l.assessment;
	const rows: KeyValueRow[] = [
		['Light & space', wrapText(a.lightAndSpace, valueWidth)],
		['Photo analysis', wrapText(a.photoAnalysis, valueWidth)],
	];
	if (a.neighborhoodAnalysis) rows.push(['Neighborhood', wrapText(a.neighborhoodAnalysis, valueWidth)]);
	if (a.tradeoffs) rows.push(['Tradeoffs', wrapText(a.tradeoffs, valueWidth)]);
	rows.push(['Reasoning', wrapText(a.reasoning, valueWidth)]);
	return rows;
}

function getNotesRows(l: Listing, valueWidth: number): KeyValueRow[] {
	if (l.notes.length === 0) return [];
	const notesText = wrapText(l.notes.join(', '), valueWidth);
	return [['Notes', notesText]];
}

function getMediaRows(l: Listing): KeyValueRow[] {
	const sat = l.mapViews?.satellite?.local ? 'Cached' : l.mapViews?.satellite?.remote ? 'Remote' : '--';
	const street = l.mapViews?.street?.local ? 'Cached' : l.mapViews?.street?.remote ? 'Remote' : '--';
	return [
		['Photos', `${l.images.length}`],
		['Floorplan', l.floorplan.remote ? 'Yes' : '--'],
		['EPC graph', l.epc.remote ? 'Yes' : '--'],
		['Satellite', sat],
		['Street', street],
	];
}

function getAgentRows(l: Listing): KeyValueRow[] {
	return [
		['Agent', formatValue(l.agent.name)],
		['Phone', formatValue(l.agent.phone)],
	];
}

function getLinksRows(l: Listing): KeyValueRow[] {
	const rows: KeyValueRow[] = [
		['Rightmove', l.url],
		['Google Maps', l.googleMapsUrl],
		['Street View', l.googleMapsStreetViewUrl],
	];
	if (l.epcSearchUrl) rows.push(['EPC Search', l.epcSearchUrl]);
	return rows;
}

function getAreaRows(l: Listing): KeyValueRow[] {
	const rows: KeyValueRow[] = [];
	rows.push(['LSOA', formatValue(l.area.lsoa.code)]);
	rows.push(['MSOA', formatValue(l.area.msoa.code)]);
	rows.push(['IMD decile', l.area.imd.decile ? `Decile ${l.area.imd.decile}` : '--']);
	rows.push(['IMD rank', l.area.imd.rank ? `${l.area.imd.rank}` : '--']);
	rows.push(['Income (BHC)', l.area.income.bhc !== null ? `£${l.area.income.bhc.toFixed(1)}k` : '--']);
	rows.push(['Income (AHC)', l.area.income.ahc !== null ? `£${l.area.income.ahc.toFixed(1)}k` : '--']);
	rows.push(['Social housing', l.area.socialHousingPct !== null ? `${l.area.socialHousingPct.toFixed(1)}%` : '--']);
	rows.push(['Flood risk', formatValue(l.area.floodRisk.level)]);
	rows.push(['Crime rate', l.area.crime.ratePer1k !== null && l.area.crime.ratePer1k !== undefined ? `${l.area.crime.ratePer1k.toFixed(2)} / 1k` : '--']);
	rows.push(['Crime 12m', l.area.crime.count12m !== null && l.area.crime.count12m !== undefined ? `${l.area.crime.count12m}` : '--']);
	return rows;
}

/** Max line width for full-width sections (matches dual-column total) */
const MAX_LINE_WIDTH = 108;

/**
 * Render detailed listing information (dual-column compact layout)
 */
export function renderDetail(listing: Listing): void {
	renderDetailHeader(listing);

	const fullValueWidth = MAX_LINE_WIDTH - KEY_WIDTH - 2;

	// Dual-column: left (Property + Details + Stations) | right (Scores + Factors + Assessment short)
	const leftRows: KeyValueRow[] = [...getPropertyRows(listing), ...getDetailsRows(listing), ...getStationsRows(listing)];
	const rightRows: KeyValueRow[] = [...getScoresRows(listing), ...getFactorsRows(listing), ...getAssessmentShortRows(listing)];
	printTwoColumns(leftRows, rightRows, { keyWidth: KEY_WIDTH });

	// Full-width: Long assessment text
	const longRows = getAssessmentLongRows(listing, fullValueWidth);
	if (longRows.length > 0) {
		subheader('Assessment');
		printKeyValues(longRows, { keyWidth: KEY_WIDTH });
	}

	// Full-width: Notes
	const notesRows = getNotesRows(listing, fullValueWidth);
	if (notesRows.length > 0) {
		subheader('Notes');
		printKeyValues(notesRows, { keyWidth: KEY_WIDTH });
	}

	// Full-width: Media, Agent, Links
	subheader('Area');
	printKeyValues(getAreaRows(listing), { keyWidth: KEY_WIDTH });

	subheader('Media & Links');
	printKeyValues([...getMediaRows(listing), ...getAgentRows(listing), ...getLinksRows(listing)], { keyWidth: KEY_WIDTH });
}

/**
 * Render statistics
 */
function renderStats(listings: Listing[]): void {
	const stats = computeStats(listings);
	section(`Listing Statistics (${stats.total} total)`);

	// By Region
	subheader('By Region');
	const regionTable = createTable([
		{ name: 'region', title: 'REGION', alignment: 'left' },
		{ name: 'count', title: 'COUNT', alignment: 'right' },
		{ name: 'percent', title: 'PERCENT', alignment: 'right' },
	]);
	for (const { region, count, percent } of stats.byRegion) {
		regionTable.addRow({ region, count, percent: formatPercent(percent) });
	}
	regionTable.printTable();
	printKeyValues([['Regions', `${stats.byRegion.length}`]], { keyWidth: 7 });

	// By Bedrooms
	subheader('By Bedrooms');
	const bedsTable = createTable([
		{ name: 'bedrooms', title: 'BEDROOMS', alignment: 'left' },
		{ name: 'count', title: 'COUNT', alignment: 'right' },
		{ name: 'percent', title: 'PERCENT', alignment: 'right' },
	]);
	for (const { bedrooms, count, percent } of stats.byBedrooms) {
		bedsTable.addRow({ bedrooms: `${bedrooms} bed`, count, percent: formatPercent(percent) });
	}
	bedsTable.printTable();

	// Score Distribution
	subheader('Score Distribution');
	const scoreDistTable = createTable([
		{ name: 'range', title: 'RANGE', alignment: 'left' },
		{ name: 'count', title: 'COUNT', alignment: 'right' },
		{ name: 'percent', title: 'PERCENT', alignment: 'right' },
	]);
	for (const { label, min, max, count, percent } of stats.scoreDistribution) {
		scoreDistTable.addRow({ range: `${label} (${min}-${max})`, count, percent: formatPercent(percent) });
	}
	scoreDistTable.printTable();

	// Price Range
	subheader('Price Range');
	const priceTable = createTable([
		{ name: 'metric', title: 'METRIC', alignment: 'left' },
		{ name: 'value', title: 'VALUE', alignment: 'right' },
	]);
	priceTable.addRow({ metric: 'Min', value: formatPrice(stats.price.min) });
	priceTable.addRow({ metric: 'Max', value: formatPrice(stats.price.max) });
	priceTable.addRow({ metric: 'Avg', value: formatPrice(stats.price.avg) });
	priceTable.addRow({ metric: 'Median', value: formatPrice(stats.price.median) });
	priceTable.printTable();

	// Score Range
	subheader('Score Range');
	const scoreTable = createTable([
		{ name: 'metric', title: 'METRIC', alignment: 'left' },
		{ name: 'value', title: 'VALUE', alignment: 'right' },
	]);
	scoreTable.addRow({ metric: 'Min', value: formatScoreWithSignal(stats.score.min) });
	scoreTable.addRow({ metric: 'Max', value: formatScoreWithSignal(stats.score.max) });
	scoreTable.addRow({ metric: 'Avg', value: formatScoreWithSignal(stats.score.avg) });
	scoreTable.addRow({ metric: 'Median', value: formatScoreWithSignal(stats.score.median) });
	scoreTable.printTable();
}

/**
 * let view list - Display listings in table format
 */
const viewList = defineCommand({
	meta: {
		name: 'list',
		description: 'Display listings in table format',
	},
	args: {
		top: {
			type: 'string',
			description: 'Limit to top N by score',
			default: '20',
		},
		'min-score': {
			type: 'string',
			description: 'Minimum score threshold (0-100)',
		},
		sort: {
			type: 'string',
			description: 'Sort by: score, price, bedrooms, date',
			default: 'score',
		},
		asc: {
			type: 'boolean',
			description: 'Ascending order (default: descending)',
			default: false,
		},
		region: {
			type: 'string',
			description: 'Filter by region name',
		},
		type: {
			type: 'string',
			description: 'Filter by property type (comma-separated: flat,terraced,semi)',
		},
	},
	async run({ args }) {
		const { listings } = loadExistingListings();

		if (listings.length === 0) {
			log.cli.warn('No listings found. Run "let fetch" first.');
			return;
		}

		const top = Number.parseInt(args.top, 10);
		const minScore = args['min-score'] ? Number.parseInt(args['min-score'], 10) : undefined;
		const sortField = parseSortField(args.sort);
		const desc = !args.asc;

		const filtered = queryListings(listings, { top: Number.isNaN(top) ? 20 : top, minScore, region: args.region, type: args.type }, sortField, desc);

		if (filtered.length === 0) {
			log.cli.warn('No listings match your filters');
			return;
		}

		log.cli.info(`Showing ${filtered.length} of ${listings.length} listings`, {
			sort: sortField,
			order: desc ? 'desc' : 'asc',
			...(minScore !== undefined && { minScore }),
			...(args.region && { region: args.region }),
			...(args.type && { type: args.type }),
		});

		renderTable(filtered);
	},
});

/**
 * let view detail <id> - View full listing details
 */
const viewDetail = defineCommand({
	meta: {
		name: 'detail',
		description: 'View full details for a specific listing',
	},
	args: {
		id: {
			type: 'positional',
			description: 'Listing ID',
			required: true,
		},
		open: {
			type: 'boolean',
			description: 'Open Rightmove URL in browser',
			default: false,
		},
	},
	async run({ args }) {
		const { listings } = loadExistingListings();

		if (listings.length === 0) {
			log.cli.warn('No listings found. Run "let fetch" first.');
			return;
		}

		const listing = findListingById(listings, args.id);

		if (!listing) {
			log.cli.error('Listing not found', { id: args.id });
			process.exit(1);
		}

		renderDetail(listing);

		if (args.open) {
			log.cli.info('Opening in browser', { url: listing.url });
			await Bun.$`open ${listing.url}`.quiet();
		}
	},
});

/**
 * let view stats - View aggregate statistics
 */
const viewStats = defineCommand({
	meta: {
		name: 'stats',
		description: 'View aggregate statistics for listings',
	},
	async run() {
		const { listings } = loadExistingListings();

		if (listings.length === 0) {
			log.cli.warn('No listings found. Run "let fetch" first.');
			return;
		}

		renderStats(listings);
	},
});

/** Valid sort fields for regions */
const VALID_REGION_SORT_FIELDS: RegionSortField[] = ['score', 'price', 'count', 'area', 'station', 'gigabit', 'garden', 'gas', 'top'];

/** Parse and validate region sort field */
function parseRegionSortField(value: string): RegionSortField {
	if (VALID_REGION_SORT_FIELDS.includes(value as RegionSortField)) {
		return value as RegionSortField;
	}
	log.cli.warn(`Invalid sort field "${value}", using "score"`, { valid: VALID_REGION_SORT_FIELDS });
	return 'score';
}

/** Render region comparison table */
function renderRegionTable(stats: RegionStats[], total: number, sortField: string, desc: boolean): void {
	section(`Region Comparison (${stats.length} regions, ${total} listings)`);

	const table = createTable([
		{ name: 'region', title: 'REGION', alignment: 'left' },
		{ name: 'count', title: 'COUNT', alignment: 'right' },
		{ name: 'score', title: 'SCORE', alignment: 'right' },
		{ name: 'avgPrice', title: 'AVG', alignment: 'right' },
		{ name: 'medPrice', title: 'MED', alignment: 'right' },
		{ name: 'range', title: 'RANGE', alignment: 'right' },
		{ name: 'epc', title: 'EPC', alignment: 'left' },
		{ name: 'area', title: 'AREA', alignment: 'right' },
		{ name: 'station', title: 'STATION', alignment: 'right' },
		{ name: 'gigabit', title: 'GIGABIT', alignment: 'right' },
		{ name: 'garden', title: 'GARDEN', alignment: 'right' },
		{ name: 'gas', title: 'GAS', alignment: 'right' },
		{ name: 'top', title: 'TOP%', alignment: 'right' },
	]);

	for (const s of stats) {
		table.addRow({
			region: truncate(s.region, 18),
			count: s.count,
			score: formatScoreWithSignal(s.avgScore),
			avgPrice: formatPrice(s.avgPrice),
			medPrice: formatPrice(s.medianPrice),
			range: `${formatPrice(s.minPrice)}-${formatPrice(s.maxPrice)}`,
			epc: s.epcTrend,
			area: s.avgArea ? `${s.avgArea}sqm` : '--',
			station: s.avgStation ? `${s.avgStation}mi` : '--',
			gigabit: formatPercent(s.gigabitPct),
			garden: formatPercent(s.gardenPct),
			gas: formatPercent(s.gasPct),
			top: formatPercent(s.topPct),
		});
	}

	table.printTable();
	printKeyValues(
		[
			['Sorted', `${sortField} (${desc ? 'desc' : 'asc'})`],
			['Top%', 'Listings scoring 85%+'],
		],
		{ keyWidth: 6 },
	);
}

/**
 * let view regions - Compare regions by aggregated metrics
 */
const viewRegions = defineCommand({
	meta: {
		name: 'regions',
		description: 'Compare regions by aggregated metrics',
	},
	args: {
		sort: {
			type: 'string',
			description: 'Sort by: score, price, count, area, station, gigabit, garden, gas, top',
			default: 'score',
		},
		asc: {
			type: 'boolean',
			description: 'Ascending order (default: descending)',
			default: false,
		},
	},
	async run({ args }) {
		const { listings } = loadExistingListings();

		if (listings.length === 0) {
			log.cli.warn('No listings found. Run "let fetch" first.');
			return;
		}

		const sortField = parseRegionSortField(args.sort);
		const desc = !args.asc;

		let stats = computeRegionStats(listings);
		stats = sortRegionStats(stats, sortField, desc);

		renderRegionTable(stats, listings.length, sortField, desc);
	},
});

/**
 * Main view command with subcommands
 */
export const viewCommand = defineCommand({
	meta: {
		name: 'view',
		description: 'Display listings and analytics',
	},
	subCommands: {
		list: viewList,
		detail: viewDetail,
		stats: viewStats,
		regions: viewRegions,
	},
});
