/**
 * Pipeline Stage 1: Fetch
 *
 * Data acquisition from Rightmove via HTML scraping or REST API.
 * Exports the public API for fetching listings and search results.
 */

// REST API
export {
	API_DELAY_MS,
	type ApiProperty,
	type ApiSearchParams,
	type ApiSearchResponse,
	type ApiSearchResult,
	buildSearchApiUrl,
	type LocationLookupResult,
	type LocationResult,
	lookupLocation,
	resetApiRateLimiter,
	searchListingsApi,
	setApiDelay,
	setApiMaxRetries,
	tokenizeLocation,
} from './api.js';
// HTML fetching
export {
	buildListingUrl,
	buildSearchUrl,
	createResettableRateLimiter,
	DEFAULT_DELAY_MS,
	type FetchResult,
	fetchWithRateLimit,
	getJitteredDelay,
	resetRateLimiter,
	setFetchDelay,
	setFetchMaxRetries,
	sleep,
} from './html.js';
// Image processing
export { downloadListingImages, generateImageFilename, getListingCacheDir, type ImageDownloadResult, type ImageEntry } from './images.js';
// Map views (satellite + street)
export {
	buildPublicMapUrl,
	// Deprecated backward compatibility exports
	buildPublicSatelliteUrl,
	fetchMapViews,
	fetchSatelliteImage,
	generateMapFilename,
	generateSatelliteFilename,
	type MapViewEntry,
	type MapViews,
	type MapViewsFetchResult,
	type MapViewType,
	type SatelliteEntry,
	type SatelliteFetchResult,
} from './maps.js';
