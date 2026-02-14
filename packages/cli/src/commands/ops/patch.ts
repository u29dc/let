/**
 * Ops command - patch listing fields with selective re-enrichment
 *
 * Override address, postcode, coordinates, or EPC data on an existing listing.
 * Automatically re-enriches downstream data affected by the change and rescores.
 */

import { paths } from '@let/core/paths';
import { type EnrichOptions, enrichListing, lookupPostcode } from '@let/core/pipeline/enrich';
import { fetchMapViews } from '@let/core/pipeline/fetch';
import { recalcAssessedScores, scoreListingsWithConfig } from '@let/core/pipeline/score';
import { findListingById } from '@let/core/pipeline/view';
import type { Listing, ListingsFile } from '@let/core/schema';
import { log } from '@let/core/utils/logger';
import { fail, isJsonMode, ok, rethrowCapture } from '../../envelope.js';
import { defineToolCommand } from '../../tool.js';
import { loadExistingListings } from '../shared-read.js';
import { loadConfigOrExit, saveListingsFile } from '../shared-write.js';

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

type PatchArgs = {
	id: string;
	address: string | undefined;
	postcode: string | undefined;
	lat: string | undefined;
	lng: string | undefined;
	'epc-rating': string | undefined;
	'floor-area': string | undefined;
	'skip-re-enrich': boolean;
	'skip-images': boolean;
};

type ChangeEntry = { from: string | number | null; to: string | number };

/** Typed changeset -- known keys avoid index-signature access issues */
type Changeset = {
	address?: ChangeEntry;
	postcode?: ChangeEntry;
	lat?: ChangeEntry;
	lng?: ChangeEntry;
	epcRating?: ChangeEntry;
	floorArea?: ChangeEntry;
};

type EpcRating = 'A' | 'B' | 'C' | 'D' | 'E' | 'F' | 'G';

const VALID_EPC_RATINGS = new Set<string>(['A', 'B', 'C', 'D', 'E', 'F', 'G']);

// ---------------------------------------------------------------------------
// Validation helpers (split to reduce cognitive complexity)
// ---------------------------------------------------------------------------

function validateHasOverride(args: PatchArgs, jsonMode: boolean, start: number): void {
	const hasOverride = args.address || args.postcode || args.lat || args.lng || args['epc-rating'] || args['floor-area'];
	if (!hasOverride) {
		if (jsonMode) fail('ops.patch', 'VALIDATION_ERROR', 'No overrides provided', 'Provide at least one of --address, --postcode, --lat, --lng, --epc-rating, --floor-area', start);
		log.cli.error('No overrides provided');
		process.exit(1);
	}
}

function validateCoordPair(args: PatchArgs, jsonMode: boolean, start: number): void {
	if ((args.lat && !args.lng) || (!args.lat && args.lng)) {
		if (jsonMode) fail('ops.patch', 'VALIDATION_ERROR', '--lat and --lng must be provided together', 'Provide both --lat and --lng', start);
		log.cli.error('--lat and --lng must be provided together');
		process.exit(1);
	}
}

function validateEpcRating(args: PatchArgs, jsonMode: boolean, start: number): void {
	if (args['epc-rating'] && !VALID_EPC_RATINGS.has(args['epc-rating'].toUpperCase())) {
		if (jsonMode) fail('ops.patch', 'VALIDATION_ERROR', `Invalid EPC rating: ${args['epc-rating']}`, 'Use A-G', start);
		log.cli.error('Invalid --epc-rating', { value: args['epc-rating'], expected: 'A-G' });
		process.exit(1);
	}
}

function validateLatLng(args: PatchArgs, jsonMode: boolean, start: number): void {
	if (args.lat) {
		const lat = Number.parseFloat(args.lat);
		if (Number.isNaN(lat) || lat < -90 || lat > 90) {
			if (jsonMode) fail('ops.patch', 'VALIDATION_ERROR', `Invalid latitude: ${args.lat}`, 'Provide a number between -90 and 90', start);
			log.cli.error('Invalid --lat value', { value: args.lat });
			process.exit(1);
		}
	}
	if (args.lng) {
		const lng = Number.parseFloat(args.lng);
		if (Number.isNaN(lng) || lng < -180 || lng > 180) {
			if (jsonMode) fail('ops.patch', 'VALIDATION_ERROR', `Invalid longitude: ${args.lng}`, 'Provide a number between -180 and 180', start);
			log.cli.error('Invalid --lng value', { value: args.lng });
			process.exit(1);
		}
	}
}

function validateFloorArea(args: PatchArgs, jsonMode: boolean, start: number): void {
	if (args['floor-area']) {
		const area = Number.parseFloat(args['floor-area']);
		if (Number.isNaN(area) || area <= 0) {
			if (jsonMode) fail('ops.patch', 'VALIDATION_ERROR', `Invalid floor area: ${args['floor-area']}`, 'Provide a positive number in sqm', start);
			log.cli.error('Invalid --floor-area value', { value: args['floor-area'] });
			process.exit(1);
		}
	}
}

function validateArgs(args: PatchArgs, jsonMode: boolean, start: number): void {
	validateHasOverride(args, jsonMode, start);
	validateCoordPair(args, jsonMode, start);
	validateEpcRating(args, jsonMode, start);
	validateLatLng(args, jsonMode, start);
	validateFloorArea(args, jsonMode, start);
}

// ---------------------------------------------------------------------------
// Changeset building
// ---------------------------------------------------------------------------

function addFieldChanges(changeset: Changeset, listing: Listing, args: PatchArgs): void {
	if (args.address && args.address !== listing.address) {
		changeset.address = { from: listing.address, to: args.address };
	}
	if (args.postcode && args.postcode !== listing.postcode) {
		changeset.postcode = { from: listing.postcode, to: args.postcode };
	}
}

function addCoordChanges(changeset: Changeset, listing: Listing, args: PatchArgs): void {
	if (args.lat && args.lng) {
		const newLat = Number.parseFloat(args.lat);
		const newLng = Number.parseFloat(args.lng);
		if (newLat !== listing.location.lat) {
			changeset.lat = { from: listing.location.lat, to: newLat };
		}
		if (newLng !== listing.location.lng) {
			changeset.lng = { from: listing.location.lng, to: newLng };
		}
	}
}

function addEpcChanges(changeset: Changeset, listing: Listing, args: PatchArgs): void {
	if (args['epc-rating']) {
		const rating = args['epc-rating'].toUpperCase();
		if (rating !== listing.epcRating) {
			changeset.epcRating = { from: listing.epcRating ?? null, to: rating };
		}
	}
	if (args['floor-area']) {
		const area = Number.parseFloat(args['floor-area']);
		if (area !== listing.floorAreaSqm) {
			changeset.floorArea = { from: listing.floorAreaSqm ?? null, to: area };
		}
	}
}

function buildChangeset(listing: Listing, args: PatchArgs): Changeset {
	const changeset: Changeset = {};
	addFieldChanges(changeset, listing, args);
	addCoordChanges(changeset, listing, args);
	addEpcChanges(changeset, listing, args);
	return changeset;
}

/** List non-undefined keys in a changeset */
function changesetKeys(c: Changeset): string[] {
	const keys: string[] = [];
	if (c.address) keys.push('address');
	if (c.postcode) keys.push('postcode');
	if (c.lat) keys.push('lat');
	if (c.lng) keys.push('lng');
	if (c.epcRating) keys.push('epcRating');
	if (c.floorArea) keys.push('floorArea');
	return keys;
}

// ---------------------------------------------------------------------------
// Google Maps URL builders (mirroring parse/index.ts private functions)
// ---------------------------------------------------------------------------

function buildGoogleMapsUrl(lat: number, lng: number, address: string, postcode: string): string {
	const place = encodeURIComponent(`${address}, ${postcode}`);
	return `https://www.google.com/maps/place/${place}/@${lat},${lng},17z/data=!3m1!1e3`;
}

function buildGoogleMapsStreetViewUrl(lat: number, lng: number): string {
	return `https://www.google.com/maps/@?api=1&map_action=pano&viewpoint=${lat},${lng}`;
}

// ---------------------------------------------------------------------------
// Field application
// ---------------------------------------------------------------------------

function applyOverrides(listing: Listing, changeset: Changeset): void {
	if (changeset.address) listing.address = changeset.address.to as string;
	if (changeset.postcode) listing.postcode = changeset.postcode.to as string;
	if (changeset.lat) listing.location.lat = changeset.lat.to as number;
	if (changeset.lng) listing.location.lng = changeset.lng.to as number;
	if (changeset.epcRating) listing.epcRating = changeset.epcRating.to as EpcRating;
	if (changeset.floorArea) listing.floorAreaSqm = changeset.floorArea.to as number;

	// Rebuild Google Maps URLs when address/postcode/coords change
	if (changeset.address || changeset.postcode || changeset.lat || changeset.lng) {
		listing.googleMapsUrl = buildGoogleMapsUrl(listing.location.lat, listing.location.lng, listing.address, listing.postcode);
		listing.googleMapsStreetViewUrl = buildGoogleMapsStreetViewUrl(listing.location.lat, listing.location.lng);
	}
}

/** Default area object matching schema defaults */
const DEFAULT_AREA = {
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
} as const;

function clearStaleEnrichment(listing: Listing, changeset: Changeset, hasDirectEpc: boolean): void {
	const postcodeChanged = changeset.postcode !== undefined;
	const addressChanged = changeset.address !== undefined;

	// Clear EPC fields if address or postcode changed (unless direct EPC override provided)
	if ((addressChanged || postcodeChanged) && !hasDirectEpc) {
		listing.epcRating = null;
		listing.floorAreaSqm = null;
		listing.epcLodgementDate = null;
		listing.epcAddressMatch = null;
		listing.epcSearchUrl = null;
	}

	// Clear broadband if postcode changed
	if (postcodeChanged) {
		listing.gigabitAvailability = null;
	}

	// Reset area if postcode changed
	if (postcodeChanged) {
		listing.area = { ...DEFAULT_AREA, crime: { ...DEFAULT_AREA.crime } };
	}
}

// ---------------------------------------------------------------------------
// Coordinate auto-resolution
// ---------------------------------------------------------------------------

/** Resolve coordinates from postcodes DB when postcode changes without explicit --lat/--lng */
function autoResolveCoords(listing: Listing, changeset: Changeset): void {
	if (!changeset.postcode || changeset.lat || changeset.lng) return;
	const lookup = lookupPostcode(changeset.postcode.to as string);
	if (lookup?.lat != null && lookup?.lng != null) {
		const prevLat = listing.location.lat;
		const prevLng = listing.location.lng;
		listing.location.lat = lookup.lat;
		listing.location.lng = lookup.lng;
		changeset.lat = { from: prevLat, to: lookup.lat };
		changeset.lng = { from: prevLng, to: lookup.lng };
		listing.googleMapsUrl = buildGoogleMapsUrl(listing.location.lat, listing.location.lng, listing.address, listing.postcode);
		listing.googleMapsStreetViewUrl = buildGoogleMapsStreetViewUrl(listing.location.lat, listing.location.lng);
		log.cli.info('Auto-resolved coordinates from postcode', { postcode: listing.postcode, lat: lookup.lat, lng: lookup.lng });
	} else {
		log.cli.warn('Could not resolve coordinates from postcode', { postcode: listing.postcode });
	}
}

// ---------------------------------------------------------------------------
// Re-enrichment
// ---------------------------------------------------------------------------

function determineReEnrichment(changeset: Changeset, hasDirectEpc: boolean, skipReEnrich: boolean): { options: EnrichOptions; stages: string[] } {
	if (skipReEnrich) return { options: { skipEpc: true, skipBroadband: true, skipArea: true, skipNotes: true }, stages: [] };

	const addressChanged = changeset.address !== undefined;
	const postcodeChanged = changeset.postcode !== undefined;
	const coordsChanged = changeset.lat !== undefined || changeset.lng !== undefined;

	const skipEpc = !(addressChanged || postcodeChanged) || hasDirectEpc;
	const skipBroadband = !postcodeChanged;
	const skipArea = !postcodeChanged;

	const stages: string[] = [];
	if (!skipEpc) stages.push('epc');
	if (!skipBroadband) stages.push('broadband');
	if (!skipArea) stages.push('area');
	if (coordsChanged) stages.push('maps');

	return {
		options: { skipEpc, skipBroadband, skipArea, skipNotes: true },
		stages,
	};
}

// ---------------------------------------------------------------------------
// Core execution (split into phases to reduce complexity)
// ---------------------------------------------------------------------------

const NULL_MAP_VIEWS = { satellite: { remote: null, local: null }, street: { remote: null, local: null } };

/** Phase: re-enrich and re-download maps */
async function applyReEnrichment(listing: Listing, changeset: Changeset, stages: string[], enrichOptions: EnrichOptions, skipImages: boolean): Promise<void> {
	const enrichStages = stages.filter((s) => s !== 'maps');
	if (enrichStages.length > 0) {
		log.cli.info('Re-enriching', { stages: enrichStages });
		await enrichListing(listing, enrichOptions);
	}

	// Re-apply direct EPC overrides after enrichment (enrichListing may overwrite them)
	if (changeset.epcRating) listing.epcRating = changeset.epcRating.to as EpcRating;
	if (changeset.floorArea) listing.floorAreaSqm = changeset.floorArea.to as number;

	if (stages.includes('maps') && !skipImages) {
		const portalId = listing.portalIds.rightmove ?? listing.id;
		const cacheDir = paths().resolved.cache;
		log.cli.info('Re-fetching map views', { lat: listing.location.lat, lng: listing.location.lng });
		const mapResult = await fetchMapViews(portalId, listing.location.lat, listing.location.lng, cacheDir);
		listing.mapViews = mapResult.success ? mapResult.mapViews : NULL_MAP_VIEWS;
	}
}

/** Phase: rescore and save */
async function rescoreAndSave(existing: { listings: Listing[]; searchUrls: string[]; locations: string[]; lastSearchTotal: number }): Promise<Listing[]> {
	const config = await loadConfigOrExit();
	const scored = scoreListingsWithConfig(existing.listings, config as unknown as Record<string, unknown>);
	recalcAssessedScores(scored);

	const output: ListingsFile = {
		updatedAt: new Date().toISOString(),
		searchUrls: existing.searchUrls,
		locations: existing.locations,
		lastSearchTotal: existing.lastSearchTotal,
		listings: scored,
	};
	await saveListingsFile(output);
	return scored;
}

type ExistingData = { listings: Listing[]; searchUrls: string[]; locations: string[]; lastSearchTotal: number };

/** Load DB and find listing, failing with appropriate error codes */
function loadAndFind(id: string, jsonMode: boolean, start: number): { existing: ExistingData; listing: Listing } {
	const existing = loadExistingListings();
	if (existing.listings.length === 0) {
		if (jsonMode) fail('ops.patch', 'NO_DATA', 'No listings in database', 'Fetch listings first with `let fetch`', start);
		log.cli.error('No listings in database');
		process.exit(1);
	}

	const listing = findListingById(existing.listings, id);
	if (!listing) {
		if (jsonMode) fail('ops.patch', 'NOT_FOUND', `Listing not found: ${id}`, 'Check the ID with `let view list`', start);
		log.cli.error('Listing not found', { id });
		process.exit(1);
	}

	return { existing, listing };
}

async function executePatch(args: PatchArgs, jsonMode: boolean, start: number): Promise<void> {
	const { existing, listing } = loadAndFind(args.id, jsonMode, start);

	const changeset = buildChangeset(listing, args);
	const keys = changesetKeys(changeset);
	if (keys.length === 0) {
		const noopData = {
			id: listing.id,
			applied: {},
			reEnriched: [],
			rescored: existing.listings.length,
			previousScore: listing.scores?._overall ?? null,
			newScore: listing.scores?._overall ?? null,
		};
		if (jsonMode) ok('ops.patch', noopData, start);
		log.cli.success('No changes needed - all values already match');
		return;
	}

	const previousScore = listing.scores?._overall ?? null;
	const hasDirectEpc = changeset.epcRating !== undefined || changeset.floorArea !== undefined;

	log.cli.info('Applying patch', { id: args.id, changes: keys });
	clearStaleEnrichment(listing, changeset, hasDirectEpc);
	applyOverrides(listing, changeset);
	autoResolveCoords(listing, changeset);

	const { options: enrichOptions, stages } = determineReEnrichment(changeset, hasDirectEpc, args['skip-re-enrich']);
	await applyReEnrichment(listing, changeset, stages, enrichOptions, args['skip-images']);

	const scored = await rescoreAndSave(existing);
	const patchedListing = findListingById(scored, args.id);
	const newScore = patchedListing?.scores?._overall ?? null;

	if (jsonMode) {
		ok(
			'ops.patch',
			{
				id: listing.id,
				applied: changeset,
				reEnriched: stages,
				rescored: scored.length,
				previousScore,
				newScore,
			},
			start,
		);
	}

	log.cli.success('Patch applied', {
		id: args.id,
		changes: keys.length,
		reEnriched: stages.length > 0 ? stages.join(', ') : 'none',
		previousScore,
		newScore,
		path: paths().derived.database,
	});
}

// ---------------------------------------------------------------------------
// Command definition
// ---------------------------------------------------------------------------

/**
 * let ops patch - Override listing fields with auto re-enrichment
 */
export const patchCommand = defineToolCommand(
	{
		name: 'ops.patch',
		command: 'let ops patch <id>',
		category: 'ops',
		outputSchema: {
			id: { type: 'string', description: 'Listing UUID' },
			applied: { type: 'object', items: 'ChangeEntry', description: 'Field changes: { field: { from, to } }' },
			reEnriched: { type: 'array', items: 'string', description: 'Re-enrichment stages run (epc, broadband, area, maps)' },
			rescored: { type: 'number', description: 'Total listings rescored' },
			previousScore: { type: 'number', description: 'Score before patch' },
			newScore: { type: 'number', description: 'Score after patch' },
		},
		idempotent: true,
		rateLimit: 'EPC API if re-enriching',
		example: 'let ops patch 172223234 --address "5 Picton Dr, Shrewsbury SY2 5WP" --postcode "SY2 5WP" --json',
	},
	{
		meta: {
			name: 'patch',
			description: 'Override listing fields (address, postcode, coords, EPC) with auto re-enrichment',
		},
		args: {
			id: { type: 'positional', description: 'Listing UUID or portal ID', required: true },
			address: { type: 'string', description: 'Override display address' },
			postcode: { type: 'string', description: 'Override postcode' },
			lat: { type: 'string', description: 'Override latitude' },
			lng: { type: 'string', description: 'Override longitude' },
			'epc-rating': { type: 'string', description: 'Direct EPC rating (A-G)' },
			'floor-area': { type: 'string', description: 'Direct floor area in sqm' },
			'skip-re-enrich': { type: 'boolean', description: 'Apply overrides without re-enrichment', default: false },
			'skip-images': { type: 'boolean', description: 'Skip map re-download on coord change', default: false },
			json: { type: 'boolean', description: 'Output as JSON envelope', default: false },
		},
		async run({ args }) {
			const start = performance.now();
			const jsonMode = isJsonMode();
			try {
				const patchArgs = args as unknown as PatchArgs;
				validateArgs(patchArgs, jsonMode, start);
				await executePatch(patchArgs, jsonMode, start);
			} catch (error) {
				rethrowCapture(error);
				const message = error instanceof Error ? error.message : String(error);
				if (jsonMode) fail('ops.patch', 'PATCH_ERROR', `Patch failed: ${message}`, 'Check listing ID and override values', start);
				log.cli.error(`Patch failed: ${message}`);
				process.exit(1);
			}
		},
	},
);
