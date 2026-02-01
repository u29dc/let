/**
 * Pipeline Stage 2: Parse
 *
 * Data extraction and transformation from raw HTML/JSON to Listing objects.
 * Exports the public API for parsing listings and search results.
 */

import { randomUUID } from 'node:crypto';
import { type Listing, ListingSchema } from '@let/core/schema';
import { log } from '@let/core/utils/logger';
import { buildListingUrl, fetchWithRateLimit } from '../fetch/index.js';
import { extractNextData, extractPageModel, getPath, isArray, isNumber, isObject, isString } from './extract.js';
import { parseListedDate, parsePrice, sanitizeForAi } from './sanitize.js';

// Re-export extraction utilities
export { extractNextData, extractPageModel, findJsonEnd, getPath, isArray, isNumber, isObject, isString, type ParseResult } from './extract.js';

// Re-export sanitization utilities
export { convertLineBreaks, decodeHtmlEntities, normalizeWhitespace, parseListedDate, parsePrice, sanitizeForAi, sanitizeHtml, stripHtmlTags } from './sanitize.js';

// =============================================================================
// RESULT TYPES
// =============================================================================

/**
 * Result of scraping a listing
 */
export type ScrapeResult = { success: true; listing: Listing } | { success: false; error: string; partial?: Partial<Listing> };

/**
 * Result of scraping search results
 */
export type SearchScrapeResult = { success: true; listingIds: string[]; totalResults: number } | { success: false; error: string };

// =============================================================================
// HELPER FUNCTIONS
// =============================================================================

/**
 * Extract broadband speed from Rightmove data
 * Returns highest available speed in Mbps
 */
export function extractBroadband(broadbandData: unknown): number | undefined {
	if (!isArray(broadbandData)) {
		log.parse.debug('Broadband data unavailable (metadata only, no speed data)');
		return undefined;
	}

	let maxSpeed = 0;
	for (const item of broadbandData) {
		if (isObject(item)) {
			const speed = item['maxDownloadSpeed'];
			if (isNumber(speed) && speed > maxSpeed) {
				maxSpeed = speed;
			}
		}
	}

	return maxSpeed > 0 ? maxSpeed : undefined;
}

/**
 * Build Google Maps URL with satellite view and marker
 */
function buildGoogleMapsUrl(lat: number, lng: number, address: string, postcode: string): string {
	const place = encodeURIComponent(`${address}, ${postcode}`);
	return `https://www.google.com/maps/place/${place}/@${lat},${lng},17z/data=!3m1!1e3`;
}

/**
 * Build Google Maps Street View URL from coordinates
 */
function buildGoogleMapsStreetViewUrl(lat: number, lng: number): string {
	return `https://www.google.com/maps/@?api=1&map_action=pano&viewpoint=${lat},${lng}`;
}

/** Extract address info from propertyData */
function extractAddress(propertyData: Record<string, unknown>): { postcode: string; address: string } {
	const outcode = getPath(propertyData, 'address.outcode');
	const incode = getPath(propertyData, 'address.incode');
	const postcode = isString(outcode) && isString(incode) ? `${outcode} ${incode}` : '';
	const displayAddress = getPath(propertyData, 'address.displayAddress');
	return { postcode, address: isString(displayAddress) ? displayAddress : '' };
}

/** Extract description from features and text */
function extractDescription(propertyData: Record<string, unknown>): string {
	const descriptionRaw = getPath(propertyData, 'text.description');
	const featuresRaw = getPath(propertyData, 'keyFeatures');
	const featureTexts: string[] = [];
	if (isArray(featuresRaw)) {
		for (const f of featuresRaw) {
			if (isString(f)) featureTexts.push(f);
		}
	}
	const descriptionText = isString(descriptionRaw) ? descriptionRaw : '';
	return sanitizeForAi(featureTexts.join(', '), descriptionText);
}

/** Image with remote URL and nullable local cache path */
type ImageEntry = { remote: string; local: string | null };

/** Extract image entries from propertyData */
function extractImages(propertyData: Record<string, unknown>): ImageEntry[] {
	const imagesRaw = getPath(propertyData, 'images');
	const images: ImageEntry[] = [];
	if (isArray(imagesRaw)) {
		for (const img of imagesRaw) {
			if (isObject(img) && isString(img['url'])) {
				images.push({ remote: img['url'], local: null });
			}
		}
	}
	return images;
}

/** Extract first URL from an array of objects with 'url' property */
function extractFirstUrl(arr: unknown): string | null {
	if (!isArray(arr) || arr.length === 0) return null;
	const first = arr[0];
	if (isObject(first) && isString(first['url'])) return first['url'];
	return null;
}

const KM_TO_MILES = 0.621371;
const METERS_TO_MILES = 0.000621371;

function normalizeStationDistance(distance: number, unit: string | null): number {
	if (!unit) return distance;

	const normalized = unit.trim().toLowerCase();
	if (normalized === 'miles' || normalized === 'mile' || normalized === 'mi') {
		return distance;
	}
	if (normalized === 'km' || normalized === 'kilometer' || normalized === 'kilometers' || normalized === 'kilometre' || normalized === 'kilometres') {
		return distance * KM_TO_MILES;
	}
	if (normalized === 'm' || normalized === 'meter' || normalized === 'meters' || normalized === 'metre' || normalized === 'metres') {
		return distance * METERS_TO_MILES;
	}

	log.parse.warn('Unknown station distance unit', { unit, distance });
	return distance;
}

/** Extract nearest stations from propertyData */
function extractStations(propertyData: Record<string, unknown>): Array<{ name: string; distance: number; unit: string }> {
	const stationsRaw = getPath(propertyData, 'nearestStations');
	const stations: Array<{ name: string; distance: number; unit: string }> = [];
	if (!isArray(stationsRaw)) return stations;

	for (const station of stationsRaw) {
		if (!isObject(station)) continue;
		const name = station['name'];
		const distance = station['distance'];
		const unit = station['unit'];
		if (isString(name) && isNumber(distance)) {
			const unitLabel = isString(unit) ? unit : 'miles';
			const normalizedDistance = normalizeStationDistance(distance, unitLabel);
			stations.push({ name, distance: normalizedDistance, unit: 'miles' });
		}
	}
	return stations.sort((a, b) => a.distance - b.distance).slice(0, 3);
}

/** Extract lettings info from propertyData */
function extractLettings(propertyData: Record<string, unknown>): { availableDate: string | null; deposit: number | null } {
	const availableDate = getPath(propertyData, 'lettings.letAvailableDate');
	const deposit = getPath(propertyData, 'lettings.deposit');
	return {
		availableDate: isString(availableDate) ? availableDate : null,
		deposit: isNumber(deposit) ? deposit : null,
	};
}

/** Extract agent info from propertyData */
function extractAgent(propertyData: Record<string, unknown>): { name: string | null; phone: string | null } {
	const agentName = getPath(propertyData, 'customer.branchDisplayName');
	const agentPhone = getPath(propertyData, 'contactInfo.telephoneNumbers.localNumber');
	return {
		name: isString(agentName) ? agentName : null,
		phone: isString(agentPhone) ? agentPhone : null,
	};
}

/** Core fields extracted from propertyData */
type CoreFields = {
	rightmoveId: string;
	lat: number;
	lng: number;
	pinType: string | null;
	price: number;
	priceStr: string;
};

/** Extract and validate required core fields */
function extractCoreFields(propertyData: Record<string, unknown>): { success: true; fields: CoreFields } | { success: false; error: string } {
	const id = getPath(propertyData, 'id');
	if (!isString(id) && !isNumber(id)) return { success: false, error: 'Property ID not found' };

	const lat = getPath(propertyData, 'location.latitude');
	const lng = getPath(propertyData, 'location.longitude');
	if (!isNumber(lat) || !isNumber(lng)) return { success: false, error: 'Location coordinates not found' };

	const priceRaw = getPath(propertyData, 'prices.primaryPrice');
	const priceStr = isString(priceRaw) ? priceRaw : '';
	const price = parsePrice(priceStr);
	if (!price) return { success: false, error: 'Price not found or invalid' };

	const pinType = getPath(propertyData, 'location.pinType');
	return {
		success: true,
		fields: {
			rightmoveId: String(id),
			lat,
			lng,
			pinType: pinType === 'ACCURATE_POINT' || pinType === 'APPROXIMATE_POINT' ? pinType : null,
			price,
			priceStr,
		},
	};
}

/** Build the complete listing data object */
function buildListingData(core: CoreFields, propertyData: Record<string, unknown>, fetchedAt: string): { data: Record<string, unknown>; extractionStatus: 'success' | 'partial' } {
	const { postcode, address } = extractAddress(propertyData);
	const description = extractDescription(propertyData);
	const images = extractImages(propertyData);
	const nearestStations = extractStations(propertyData);

	const bedrooms = getPath(propertyData, 'bedrooms');
	const bathrooms = getPath(propertyData, 'bathrooms');
	const propertyType = getPath(propertyData, 'propertySubType');
	const updateReason = getPath(propertyData, 'listingHistory.listingUpdateReason');

	const hasAllOptional = postcode && description && images.length > 0 && nearestStations.length > 0;

	const data = {
		id: randomUUID(),
		portalIds: { rightmove: core.rightmoveId },
		uprn: null,
		uprnSource: null,
		uprnConfidence: null,
		url: buildListingUrl(core.rightmoveId),
		location: { lat: core.lat, lng: core.lng, pinType: core.pinType },
		postcode,
		address,
		region: null,
		googleMapsUrl: buildGoogleMapsUrl(core.lat, core.lng, address, postcode),
		googleMapsStreetViewUrl: buildGoogleMapsStreetViewUrl(core.lat, core.lng),
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
		price: core.price,
		priceDisplay: core.priceStr || `£${core.price} pcm`,
		bedrooms: isNumber(bedrooms) ? bedrooms : 0,
		bathrooms: isNumber(bathrooms) && bathrooms >= 1 ? bathrooms : 1,
		propertyType: isString(propertyType) ? propertyType : 'Unknown',
		description,
		notes: [],
		images,
		floorplan: { remote: extractFirstUrl(getPath(propertyData, 'floorplans')), local: null },
		epc: { remote: extractFirstUrl(getPath(propertyData, 'epcGraphs')), local: null },
		epcRating: null,
		floorAreaSqm: null,
		epcLodgementDate: null,
		epcAddressMatch: null,
		epcSearchUrl: postcode ? `https://find-energy-certificate.service.gov.uk/find-a-certificate/search-by-postcode?postcode=${encodeURIComponent(postcode)}` : null,
		nearestStations,
		gigabitAvailability: null,
		listedDate: isString(updateReason) ? (parseListedDate(updateReason) ?? null) : null,
		lettings: extractLettings(propertyData),
		agent: extractAgent(propertyData),
		assessment: null,
		assessedAt: null,
		assessedScore: null,
		scores: null,
		fetchedAt,
		extractionStatus: hasAllOptional ? 'success' : 'partial',
		status: 'active',
	};

	return { data, extractionStatus: hasAllOptional ? 'success' : 'partial' };
}

// =============================================================================
// PUBLIC API
// =============================================================================

/**
 * Transform PAGE_MODEL data into a Listing
 */
export function transformPageModel(data: unknown, fetchedAt: string): ScrapeResult {
	if (!isObject(data)) return { success: false, error: 'PAGE_MODEL is not an object' };

	const propertyData = getPath(data, 'propertyData');
	if (!isObject(propertyData)) return { success: false, error: 'propertyData not found in PAGE_MODEL' };

	const coreResult = extractCoreFields(propertyData);
	if (!coreResult.success) return { success: false, error: coreResult.error };

	const { data: listingData } = buildListingData(coreResult.fields, propertyData, fetchedAt);

	const result = ListingSchema.safeParse(listingData);
	if (!result.success) {
		return { success: false, error: `Validation failed: ${result.error.message}`, partial: listingData as Partial<Listing> };
	}

	return { success: true, listing: result.data };
}

/**
 * Scrape a single listing by ID
 *
 * @param id - Rightmove listing ID
 * @param html - Optional pre-fetched HTML (for dev mode / fixtures)
 * @returns Validated Listing or error
 */
export async function scrapeListing(id: string, html?: string): Promise<ScrapeResult> {
	const fetchedAt = new Date().toISOString();

	let pageHtml = html;
	if (!pageHtml) {
		const url = buildListingUrl(id);
		const fetchResult = await fetchWithRateLimit(url);
		if (!fetchResult.success) {
			return { success: false, error: `Fetch failed: ${fetchResult.error}` };
		}
		pageHtml = fetchResult.html;
	}

	const parseResult = extractPageModel(pageHtml);
	if (!parseResult.success) {
		return { success: false, error: parseResult.error };
	}

	return transformPageModel(parseResult.data, fetchedAt);
}

/**
 * Scrape search results page to get listing IDs
 *
 * @param html - HTML of search results page
 * @returns Array of listing IDs or error
 */
export function scrapeSearchResults(html: string): SearchScrapeResult {
	const parseResult = extractNextData(html);
	if (!parseResult.success) {
		return { success: false, error: parseResult.error };
	}

	const data = parseResult.data;
	const properties = getPath(data, 'props.pageProps.searchResults.properties');

	if (!isArray(properties)) {
		return { success: false, error: 'Properties array not found in search results' };
	}

	const listingIds: string[] = [];
	for (const prop of properties) {
		if (isObject(prop)) {
			const id = prop['id'];
			if (isNumber(id) || isString(id)) {
				listingIds.push(String(id));
			}
		}
	}

	const resultCount = getPath(data, 'props.pageProps.searchResults.resultCount');
	const totalResults = isString(resultCount) ? Number.parseInt(resultCount.replace(/,/g, ''), 10) : isNumber(resultCount) ? resultCount : listingIds.length;

	return { success: true, listingIds, totalResults };
}
