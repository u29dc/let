/**
 * Raw factor extraction from listings
 */

import type { Listing } from '@let/core/schema';
import { normalizePropertyType } from '../math/utilities.js';
import { extractNameFromAddress, extractNameFromRegion, matchRegionName } from '../regions.js';
import type { GardenType, HeatingType, PetPolicy, RawFactors } from '../types.js';

/** Detect garden type from listing description and notes */
export function detectGardenType(listing: Listing): GardenType {
	const text = `${listing.description} ${listing.notes.join(' ')}`.toLowerCase();

	if (/\bno garden\b/.test(text)) {
		return 'none';
	}

	if (/\b(shared|communal) garden\b/.test(text)) {
		return 'shared';
	}

	if (/\b(private|enclosed|rear|front|south[- ]facing|back|west[- ]facing|large|mature) garden\b/.test(text)) {
		return 'private';
	}

	if (/\bgarden\b/.test(text)) {
		return 'private';
	}

	if (/\b(patio|courtyard|outside space|outdoor space)\b/.test(text)) {
		return 'shared';
	}

	return 'none';
}

/** Detect heating type from listing description and notes */
export function detectHeatingType(listing: Listing): HeatingType {
	const text = `${listing.description} ${listing.notes.join(' ')}`.toLowerCase();

	if (/\b(gas central heating|gas heating|gas ch|gas fired|gas boiler|combi boiler)\b/.test(text)) {
		return 'gas';
	}

	if (/\b(electric heating|storage heaters?|electric radiators?|no gas|all electric)\b/.test(text)) {
		return 'electric';
	}

	if (/\bcentral heating\b/.test(text) && !/\bno gas\b/.test(text)) {
		return 'gas';
	}

	return 'unknown';
}

/** Detect pet policy from listing description and notes */
export function detectPetPolicy(listing: Listing): PetPolicy {
	const text = `${listing.description} ${listing.notes.join(' ')}`.toLowerCase();

	if (/\b(pets? (allowed|welcome|considered|friendly|negotiable)|pet[- ]friendly)\b/.test(text)) {
		return 'yes';
	}

	if (/\b(no pets?|pets? not allowed|sorry no pets?)\b/.test(text)) {
		return 'no';
	}

	return 'unknown';
}

/** Extract region name from listing */
export function extractRegionName(listing: Listing, regions?: string[]): string | null {
	if (listing.region) {
		const regionName = extractNameFromRegion(listing.region);
		if (!regions) {
			return regionName;
		}
		return matchRegionName(regionName, regions) ?? regionName;
	}
	return extractNameFromAddress(listing.address, regions);
}

/** Get nearest station distance in miles */
export function getNearestStationDistance(listing: Listing): number | null {
	if (listing.nearestStations.length === 0) {
		return null;
	}
	const nearest = listing.nearestStations[0];
	if (!nearest) return null;
	return nearest.distance;
}

/** Extract raw factors from a listing */
export function extractRawFactors(listing: Listing, regions?: string[]): RawFactors {
	return {
		monthlyRent: listing.price,
		floorAreaSqm: listing.floorAreaSqm ?? null,
		epcBand: listing.epcRating ?? null,
		bedrooms: listing.bedrooms,
		stationMiles: getNearestStationDistance(listing),
		gigabitPct: listing.gigabitAvailability ?? null,
		regionName: extractRegionName(listing, regions),
		gardenType: detectGardenType(listing),
		heatingType: detectHeatingType(listing),
		petPolicy: detectPetPolicy(listing),
		propertyType: normalizePropertyType(listing.propertyType),
		imdDecile: listing.area.imd.decile ?? null,
		crimeRatePer1k: listing.area.crime.ratePer1k ?? null,
	};
}
