/**
 * Assess command - view or submit AI assessment for listings
 */

import { calculateAssessedScore, normalizeAssessment } from '@let/core/pipeline/assess';
import { findListingById } from '@let/core/pipeline/view';
import type { Assessment, Listing, ListingsFile } from '@let/core/schema';
import { AssessmentSchema } from '@let/core/schema';
import { log } from '@let/core/utils/logger';
import { defineCommand } from 'citty';
import { colorStatus, createTable, dim, formatPercent, formatPrice, formatScoreWithSignal, formatValue, print, printKeyValues, printList, section, subheader } from '../output/index.js';
import { loadExistingListings, saveListingsFile } from './shared.js';

/** Print basic listing info */
function printBasicInfo(listing: Listing): void {
	const header = listing.address.includes(listing.postcode) ? listing.address : `${listing.address}, ${listing.postcode}`;
	const displayId = listing.portalIds.rightmove ?? listing.id;
	section(`${displayId} | ${header}`);
	subheader('Property');
	printKeyValues(
		[
			['Type', formatValue(listing.propertyType)],
			['Beds', formatValue(listing.bedrooms)],
			['Baths', formatValue(listing.bathrooms)],
			['Price', formatValue(listing.priceDisplay)],
			['Deposit', listing.lettings.deposit ? formatPrice(listing.lettings.deposit) : '--'],
			['Available', formatValue(listing.lettings.availableDate)],
			['Status', colorStatus(listing.status)],
		],
		{ keyWidth: 10 },
	);
	subheader('Details');
	printKeyValues(
		[
			['Floor Area', listing.floorAreaSqm ? `${listing.floorAreaSqm} sqm` : '--'],
			['EPC', formatValue(listing.epcRating)],
			['Broadband', listing.gigabitAvailability !== null ? `${formatPercent(listing.gigabitAvailability)} gigabit` : '--'],
			['Listed', formatValue(listing.listedDate)],
			['Region', formatValue(listing.region)],
			['Coordinates', `${listing.location.lat.toFixed(4)}, ${listing.location.lng.toFixed(4)}`],
		],
		{ keyWidth: 11 },
	);
}

/** Print score breakdown */
function printScores(listing: Listing): void {
	if (!listing.scores) return;
	subheader('Scores');
	printKeyValues(
		[
			['Overall', formatScoreWithSignal(listing.scores._overall)],
			['Confidence', formatPercent(listing.scores.confidence * 100)],
			['Affordability', formatScoreWithSignal(listing.scores.affordability)],
			['Location', formatScoreWithSignal(listing.scores.location)],
			['Liveability', formatScoreWithSignal(listing.scores.liveability)],
		],
		{ keyWidth: 12 },
	);
}

/** Print assessment schema help */
function printAssessmentHelp(): void {
	subheader('Assessment Schema');
	printKeyValues([['Submit', 'let assess <id> --json \'{"maintenance": "good", ...}\'']], { keyWidth: 6 });
	print(dim('Required'));
	printKeyValues(
		[
			['maintenance', '"excellent" | "good" | "fair" | "poor"'],
			['lightAndSpace', 'string (describe natural light, room sizes, feel)'],
			['photoAnalysis', "string (photo quality, what's shown/hidden)"],
			['recommendation', '"strong-recommend" | "recommend" | "neutral" | "avoid"'],
			['familySuitability', '"excellent" | "good" | "fair" | "poor"'],
			['reasoning', 'string (why this recommendation)'],
			['scoreAdjustment', 'number (-30 to +30, manual score adjustment)'],
		],
		{ keyWidth: 18, indent: 2 },
	);
	print(dim('Optional'));
	printKeyValues(
		[
			['tradeoffs', 'string (compensating factors, workarounds)'],
			['neighborhoodAnalysis', 'string (from satellite: parking, gardens, roads, density)'],
		],
		{ keyWidth: 18, indent: 2 },
	);
}

function printStations(listing: Listing): void {
	if (listing.nearestStations.length === 0) return;
	subheader('Stations');
	const items = listing.nearestStations.slice(0, 3).map((station) => `${station.name} (${station.distance.toFixed(1)}mi)`);
	printList(items);
}

function printNotes(listing: Listing): void {
	if (listing.notes.length === 0) return;
	subheader('Notes');
	printList(listing.notes);
}

function printImages(listing: Listing): void {
	subheader('Images');
	if (listing.images.length === 0) {
		print('--');
		return;
	}
	printList(listing.images.slice(0, 10).map((img) => img.remote));
	if (listing.images.length > 10) {
		printKeyValues([['More', `${listing.images.length - 10}`]], { keyWidth: 4 });
	}
}

function printMapViews(listing: Listing): void {
	const token = process.env['MAPBOX_ACCESS_TOKEN'] ?? 'YOUR_TOKEN';
	const hasSatellite = listing.mapViews?.satellite?.remote;
	const hasStreet = listing.mapViews?.street?.remote;

	if (!hasSatellite && !hasStreet) return;

	subheader('Map Views');
	const rows: [string, string][] = [];
	if (hasSatellite) {
		rows.push(['Satellite', `${listing.mapViews?.satellite?.remote}?access_token=${token}`]);
	}
	if (hasStreet) {
		rows.push(['Street', `${listing.mapViews?.street?.remote}?access_token=${token}`]);
	}
	printKeyValues(rows, { keyWidth: 9 });
	printKeyValues([['Analyze', 'parking, gardens, road type, density, nearby commercial/industrial, POIs']], { keyWidth: 9 });
}

function printLinks(listing: Listing): void {
	subheader('Links');
	const rows: [string, string][] = [
		['Rightmove', listing.url],
		['Google Maps', listing.googleMapsUrl],
		['Street View', listing.googleMapsStreetViewUrl],
	];
	if (listing.epcSearchUrl) rows.push(['EPC Search', listing.epcSearchUrl]);
	printKeyValues(rows, { keyWidth: 11 });
}

/** Display listing details for AI assessment */
function displayListingForAssessment(listing: Listing): void {
	printBasicInfo(listing);
	printScores(listing);
	printStations(listing);
	printNotes(listing);
	printImages(listing);
	if (listing.floorplan.remote) {
		subheader('Floorplan');
		printKeyValues([['URL', listing.floorplan.remote]], { keyWidth: 3 });
	}
	printMapViews(listing);
	printLinks(listing);
	subheader('Description');
	print(listing.description.slice(0, 1000) + (listing.description.length > 1000 ? '...' : ''));
	printAssessmentHelp();
}

/** Display top N listings that need assessment */
function displayListingsForAssessment(listings: Listing[], top: number): void {
	const unassessed = listings.filter((l) => !l.assessment && l.scores).sort((a, b) => (b.scores?._overall ?? 0) - (a.scores?._overall ?? 0));
	section(`Top ${Math.min(top, unassessed.length)} Listings Needing Assessment`);
	const table = createTable([
		{ name: 'rank', title: 'RANK', alignment: 'right' },
		{ name: 'id', title: 'ID', alignment: 'left' },
		{ name: 'score', title: 'SCORE', alignment: 'right' },
		{ name: 'address', title: 'ADDRESS', alignment: 'left' },
	]);
	for (const [i, listing] of unassessed.slice(0, top).entries()) {
		const displayId = listing.portalIds.rightmove ?? listing.id;
		table.addRow({
			rank: i + 1,
			id: displayId,
			score: formatScoreWithSignal(listing.scores?._overall ?? null),
			address: listing.address,
		});
	}
	table.printTable();
	printKeyValues(
		[
			['Total', `${unassessed.length}`],
			['Next', 'let assess <id> to view details'],
		],
		{ keyWidth: 5 },
	);
}

/**
 * let assess - View or submit AI assessment for listings
 */
export const assessCommand = defineCommand({
	meta: {
		name: 'assess',
		description: 'View listing details or submit AI assessment',
	},
	args: {
		id: {
			type: 'positional',
			description: 'Listing ID to assess',
			required: false,
		},
		json: {
			type: 'string',
			description: 'Assessment JSON to submit',
		},
		top: {
			type: 'string',
			description: 'Show top N listings needing assessment',
		},
	},
	async run({ args }) {
		const existing = loadExistingListings();

		// Show top listings needing assessment
		if (args.top) {
			const topN = Number.parseInt(args.top, 10);
			if (Number.isNaN(topN) || topN < 1) {
				log.cli.error('Invalid --top value', { value: args.top });
				return;
			}
			displayListingsForAssessment(existing.listings, topN);
			return;
		}

		// Ensure we have a listing ID
		const listingId = args.id;
		if (!listingId) {
			log.cli.error('Missing listing ID', { usage: 'let assess <id> [--json \'{"..."}\']\n       let assess --top 10' });
			return;
		}

		const listing = findListingById(existing.listings, listingId);
		if (!listing) {
			log.cli.error('Listing not found', { id: listingId });
			return;
		}

		// Submit assessment
		if (args.json) {
			const validated = AssessmentSchema.safeParse(JSON.parse(args.json));
			if (!validated.success) {
				log.cli.error('Invalid assessment JSON', {
					errors: validated.error.issues.map((i) => `${i.path.join('.')}: ${i.message}`),
				});
				printKeyValues([['See', 'let assess <id> for required schema']], { keyWidth: 3 });
				return;
			}

			const algoScore = listing.scores?._overall ?? 0;
			const assessedScore = calculateAssessedScore(algoScore, validated.data as Assessment);

			listing.assessment = normalizeAssessment(validated.data as Assessment);
			listing.assessedAt = new Date().toISOString();
			listing.assessedScore = assessedScore;

			// Save to file
			const output: ListingsFile = {
				updatedAt: new Date().toISOString(),
				searchUrls: existing.searchUrls,
				locations: existing.locations,
				lastSearchTotal: existing.lastSearchTotal,
				listings: existing.listings,
			};

			await saveListingsFile(output);
			log.cli.success('Assessment saved', {
				id: listingId,
				recommendation: validated.data.recommendation,
				algoScore,
				assessedScore,
			});
			return;
		}

		// Display listing for assessment
		displayListingForAssessment(listing);
	},
});
