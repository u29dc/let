/**
 * Rightmove REST API client
 *
 * Provides direct access to Rightmove's internal APIs:
 * - TypeAhead API for location lookup
 * - Search API for listing queries
 */

import { log } from '@let/core/utils/logger';
import { createResettableRateLimiter, sleep } from './html.js';

/** Default delay between API requests (milliseconds) */
export const API_DELAY_MS = 1000;

/** Delay between retries (milliseconds) */
const API_RETRY_DELAY_MS = 2000;

/** HTTP headers for API requests */
const API_HEADERS = {
	'User-Agent': 'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36',
	Accept: 'application/json',
	'Accept-Language': 'en-GB,en;q=0.9',
};

/** Concurrency-safe rate limiter for API requests */
let apiRateLimiter = createResettableRateLimiter(API_DELAY_MS);

/** Maximum retries for transient errors */
let apiMaxRetries = 2;

/**
 * Set the API delay (shared with HTML fetch delay)
 */
export function setApiDelay(delayMs: number): void {
	apiRateLimiter = createResettableRateLimiter(delayMs);
}

/**
 * Set maximum retries for API fetch operations
 */
export function setApiMaxRetries(value: number): void {
	const normalized = Math.max(1, Math.floor(value));
	apiMaxRetries = normalized;
}

/**
 * Check if an HTTP status code is retryable (5xx or 429)
 */
function isRetryableStatus(status: number): boolean {
	return status === 429 || status >= 500;
}

/** Calculate retry delay, respecting Retry-After header when present */
function getRetryDelay(response?: Response): number {
	const retryAfter = response?.headers.get('Retry-After');
	if (retryAfter) {
		const seconds = Number.parseInt(retryAfter, 10);
		if (!Number.isNaN(seconds)) {
			return seconds * 1000;
		}
	}
	return API_RETRY_DELAY_MS;
}

/**
 * Location lookup result from TypeAhead API
 */
export type LocationResult = {
	displayName: string;
	locationIdentifier: string;
	normalizedSearchTerm: string;
};

/**
 * Property from Search API
 */
export type ApiProperty = {
	id: number;
	bedrooms: number;
	bathrooms: number;
	numberOfImages: number;
	numberOfFloorplans: number;
	numberOfVirtualTours: number;
	summary: string;
	displayAddress: string;
	countryCode: string;
	location: {
		latitude: number;
		longitude: number;
	};
	propertyImages: {
		images: Array<{
			srcUrl: string;
			url: string;
		}>;
		mainImageSrc: string;
		mainMapImageSrc: string;
	};
	propertySubType: string;
	listingUpdate: {
		listingUpdateReason: string;
		listingUpdateDate: string;
	};
	premiumListing: boolean;
	featuredProperty: boolean;
	price: {
		amount: number;
		frequency: string;
		currencyCode: string;
		displayPrices: Array<{
			displayPrice: string;
			displayPriceQualifier: string;
		}>;
	};
	customer: {
		branchId: number;
		brandPlusLogoURI: string;
		contactTelephone: string;
		branchDisplayName: string;
		branchName: string;
		branchLandingPageUrl: string;
		development: boolean;
		showReducedProperties: boolean;
		commercial: boolean;
		showOnMap: boolean;
		brandPlusLogoUrl: string;
	};
	commercial: boolean;
	development: boolean;
	residential: boolean;
	students: boolean;
	auction: boolean;
	feesApply: boolean;
	feesApplyText: string | null;
	displaySize: string;
	showOnMap: boolean;
	propertyUrl: string;
	contactUrl: string;
	staticMapUrl: string | null;
	channel: string;
	firstVisibleDate: string;
	keywords: string[];
	keywordMatchType: string;
	saved: boolean | null;
	hidden: boolean | null;
	onlineViewingsAvailable: boolean;
	lozengeModel: {
		matchingLozenges: unknown[];
	};
	hasBrandPlus: boolean;
	displayStatus: string;
	formattedBranchName: string;
	addedOrReduced: string;
	heading: string;
	isRecent: boolean;
	productLabel: {
		productLabelText: string | null;
		spotlightLabel: boolean;
	};
};

/**
 * Search API response
 */
export type ApiSearchResponse = {
	resultCount: string;
	searchResultsCount: string;
	locationIdentifier: string;
	properties: ApiProperty[];
};

/**
 * Result of Search API call
 */
export type ApiSearchResult = { success: true; properties: ApiProperty[]; totalResults: number; listingIds: string[] } | { success: false; error: string };

/**
 * Result of Location lookup
 */
export type LocationLookupResult = { success: true; locations: LocationResult[] } | { success: false; error: string };

/**
 * Tokenize a location name for the TypeAhead API
 *
 * Rightmove uses 2-character tokens separated by "/"
 * "York" -> "YO/RK"
 * "Newcastle" -> "NE/WC/AS/TL/E"
 */
export function tokenizeLocation(name: string): string {
	const cleaned = name.toUpperCase().replace(/[^A-Z]/g, '');
	const tokens: string[] = [];

	for (let i = 0; i < cleaned.length; i += 2) {
		if (i + 1 < cleaned.length) {
			tokens.push(cleaned.slice(i, i + 2));
		} else {
			tokens.push(cleaned.slice(i));
		}
	}

	return tokens.join('/');
}

/** Result type for apiFetch */
type ApiFetchResult<T> = { success: true; data: T } | { success: false; error: string; status?: number };

/**
 * Rate-limited API fetch helper with retry logic
 */
async function apiFetch<T>(url: string): Promise<ApiFetchResult<T>> {
	await apiRateLimiter.throttle();

	let lastError = 'Unknown error';
	let lastStatus: number | undefined;
	let retryDelayMs = API_RETRY_DELAY_MS;

	for (let attempt = 1; attempt <= apiMaxRetries; attempt++) {
		try {
			const response = await fetch(url, { headers: API_HEADERS });

			if (response.ok) {
				return { success: true, data: (await response.json()) as T };
			}

			if (!isRetryableStatus(response.status)) {
				return { success: false, error: `HTTP ${response.status}: ${response.statusText}`, status: response.status };
			}

			lastError = `HTTP ${response.status}: ${response.statusText}`;
			lastStatus = response.status;
			retryDelayMs = getRetryDelay(response);
			log.fetch.warn('API request failed, retrying', { status: response.status, attempt, maxRetries: apiMaxRetries });
		} catch (e) {
			lastError = e instanceof Error ? e.message : 'Network error';
			retryDelayMs = API_RETRY_DELAY_MS;
			log.fetch.warn('API network error, retrying', { error: lastError, attempt, maxRetries: apiMaxRetries });
		}

		if (attempt < apiMaxRetries) {
			await sleep(retryDelayMs);
		}
	}

	const result: ApiFetchResult<T> = { success: false, error: lastError };
	if (lastStatus !== undefined) {
		(result as { status?: number }).status = lastStatus;
	}
	return result;
}

/**
 * Lookup location identifiers by city name
 *
 * @param cityName - City name to search for (e.g., "York", "Newcastle")
 * @returns Array of matching locations with identifiers
 */
export async function lookupLocation(cityName: string): Promise<LocationLookupResult> {
	const tokenized = tokenizeLocation(cityName);
	const url = `https://www.rightmove.co.uk/typeAhead/uknostreet/${tokenized}/`;

	log.fetch.debug('Looking up location', { cityName, tokenized, url });

	const result = await apiFetch<{
		typeAheadLocations: Array<{
			displayName: string;
			locationIdentifier: string;
			normalisedSearchTerm: string;
		}>;
	}>(url);

	if (!result.success) {
		return { success: false, error: result.error };
	}

	const locations: LocationResult[] = result.data.typeAheadLocations.map((loc) => ({
		displayName: loc.displayName,
		locationIdentifier: loc.locationIdentifier,
		normalizedSearchTerm: loc.normalisedSearchTerm,
	}));

	return { success: true, locations };
}

/**
 * Search parameters for API
 */
export type ApiSearchParams = {
	locationIdentifier: string;
	minBedrooms?: number;
	maxBedrooms?: number;
	minPrice?: number;
	maxPrice?: number;
	propertyTypes?: string[];
	includeLetAgreed?: boolean;
	radius?: number;
	dontShow?: string[];
	mustHave?: string[];
	index?: number;
	numberOfPropertiesPerPage?: number;
};

/**
 * Build Search API URL from parameters
 */
export function buildSearchApiUrl(params: ApiSearchParams): string {
	const base = 'https://www.rightmove.co.uk/api/_search';
	const searchParams = new URLSearchParams();

	searchParams.set('locationIdentifier', params.locationIdentifier);
	searchParams.set('numberOfPropertiesPerPage', String(params.numberOfPropertiesPerPage ?? 24));
	searchParams.set('radius', String(params.radius ?? 0));
	searchParams.set('sortType', '6');
	searchParams.set('index', String(params.index ?? 0));
	searchParams.set('includeSSTC', 'false');
	searchParams.set('viewType', 'LIST');
	searchParams.set('channel', 'RENT');
	searchParams.set('areaSizeUnit', 'sqft');
	searchParams.set('currencyCode', 'GBP');
	searchParams.set('isFetching', 'false');

	if (params.minBedrooms !== undefined) {
		searchParams.set('minBedrooms', params.minBedrooms.toString());
	}
	if (params.maxBedrooms !== undefined) {
		searchParams.set('maxBedrooms', params.maxBedrooms.toString());
	}
	if (params.minPrice !== undefined) {
		searchParams.set('minPrice', params.minPrice.toString());
	}
	if (params.maxPrice !== undefined) {
		searchParams.set('maxPrice', params.maxPrice.toString());
	}
	if (params.propertyTypes?.length) {
		for (const type of params.propertyTypes) {
			searchParams.append('propertyTypes', type);
		}
	}
	if (params.includeLetAgreed !== undefined) {
		searchParams.set('includeLetAgreed', params.includeLetAgreed.toString());
	}
	if (params.dontShow?.length) {
		searchParams.set('dontShow', params.dontShow.join(','));
	}
	if (params.mustHave?.length) {
		searchParams.set('mustHave', params.mustHave.join(','));
	}

	return `${base}?${searchParams.toString()}`;
}

/**
 * Search listings via Rightmove API
 *
 * @param params - Search parameters
 * @returns Array of properties with listing IDs
 */
export async function searchListingsApi(params: ApiSearchParams): Promise<ApiSearchResult> {
	const url = buildSearchApiUrl(params);

	log.fetch.info('Searching via API', { location: params.locationIdentifier });
	log.fetch.debug('API URL', { url });

	const result = await apiFetch<ApiSearchResponse>(url);

	if (!result.success) {
		return { success: false, error: result.error };
	}

	const properties = result.data.properties;
	const totalResults = Number.parseInt(result.data.resultCount?.replace(/,/g, '') ?? '0', 10);
	const listingIds = properties.map((p) => String(p.id));

	log.fetch.success('API search complete', {
		total: totalResults,
		returned: properties.length,
	});

	return {
		success: true,
		properties,
		totalResults,
		listingIds,
	};
}

/**
 * Reset API rate limiter (useful for testing)
 */
export function resetApiRateLimiter(): void {
	apiRateLimiter.reset();
}
