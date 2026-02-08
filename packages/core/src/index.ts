/**
 * @let/core - Property Search Core Library
 *
 * Clean exports for the property search and scoring system.
 */

// =============================================================================
// PATHS
// =============================================================================

export {
	type DerivedPaths,
	type PathOverrides,
	paths,
	type ResolvedPaths,
	resetPaths,
	resolvePaths,
} from './paths.js';

// =============================================================================
// SCHEMA
// =============================================================================

export {
	type Listing,
	ListingSchema,
	type ListingsFile,
	ListingsFileSchema,
	type SearchResults,
	SearchResultsSchema,
} from './schema/index.js';

// =============================================================================
// PIPELINE
// =============================================================================

// Enrich stage
export {
	type AreaEnrichmentResult,
	applyEpcToListing,
	type BroadbandResult,
	closeAreaDbs,
	type EnrichNotesResult,
	type EnrichOptions,
	type EnrichResult,
	type EpcApiResult,
	type EpcEnrichmentResult,
	type EpcRecord,
	enrichListing,
	enrichListingArea,
	enrichListingNotes,
	enrichListings,
	enrichWithEpc,
	extractNotes,
	fetchEpcByPostcode,
	lookupBroadband,
	lookupPostcode,
} from './pipeline/enrich/index.js';
// Fetch stage
export {
	type ApiSearchParams,
	type ApiSearchResult,
	buildListingUrl,
	buildSearchUrl,
	DEFAULT_DELAY_MS,
	type FetchResult,
	fetchWithRateLimit,
	type LocationLookupResult,
	lookupLocation,
	resetRateLimiter,
	searchListingsApi,
	setApiDelay,
	setApiMaxRetries,
	setFetchDelay,
	setFetchMaxRetries,
} from './pipeline/fetch/index.js';
// Parse stage
export {
	extractNextData,
	extractPageModel,
	getPath,
	isArray,
	isNumber,
	isObject,
	isString,
	type ParseResult,
	parseListedDate,
	parsePrice,
	type ScrapeResult,
	type SearchScrapeResult,
	sanitizeForAi,
	sanitizeHtml,
	scrapeListing,
	scrapeSearchResults,
	transformPageModel,
} from './pipeline/parse/index.js';

// Score stage
export {
	type AffordabilityConfig,
	aggregateScores,
	broadbandUtility,
	buildPercentileContext,
	buildScoringContext,
	type CompositeScores,
	type CompositeWeights,
	type ConfidenceMetadata,
	calculateAffordability,
	calculateCompositeImpact,
	calculateConfidence,
	calculateLiveability,
	calculateLocation,
	calculatePenalties,
	calculatePercentile,
	calculateRawScore,
	calculateStats,
	clamp,
	DEFAULT_SCORING_CONFIG,
	describeConfidence,
	detectGardenType,
	detectHeatingType,
	detectPetPolicy,
	type EpcBand,
	epcBandToNumeric,
	explainPenalties,
	exponentialDecay,
	extractNameFromAddress,
	extractNameFromRegion,
	extractRawFactors,
	extractRegionName,
	floorAreaUtility,
	type GardenType,
	getAffordabilityBreakdown,
	getHeatingCostEstimate,
	getLiveabilityBreakdown,
	getLocationBreakdown,
	getNearestStationDistance,
	type HeatingType,
	inverseLerp,
	type LiveabilityConfig,
	type LocationConfig,
	lerp,
	type NormalizedFactors,
	normalizeFactors,
	normalizePropertyType,
	type PenaltyConfig,
	type PenaltyMultipliers,
	type PercentileContext,
	type PetPolicy,
	percentileToLabel,
	type RawFactors,
	roundTo,
	type ScoreContextMetadata,
	type ScoredListing,
	type ScoreFactors,
	type Scores,
	type ScoringConfig,
	type ScoringContext,
	type ScoringResult,
	type StatsSummary,
	scoreListings,
	scoreListingsWithConfig,
	scoreSingleListing,
	sigmoid,
	sigmoidThreshold,
	stationProximityUtility,
	weightedArithmeticMean,
	weightedGeometricMean,
} from './pipeline/score/index.js';

// =============================================================================
// ASSESS
// =============================================================================

export { calculateAssessedScore, normalizeAssessment } from './pipeline/assess/index.js';

// =============================================================================
// CONFIG
// =============================================================================

export {
	type Config,
	ConfigSchema,
	type FetchConfig,
	type Location,
	loadConfig,
	loadScoringConfig,
	parseScoringConfig,
	resetConfigCache,
	ScoringConfigSchema,
	type SearchConfig,
	type SearchFilters,
} from './config/index.js';

// =============================================================================
// VIEW
// =============================================================================

export {
	computeRegionStats,
	computeStats,
	filterByMinScore,
	filterByRegion,
	filterByType,
	filterListings,
	findListingById,
	formatStation,
	formatTableRow,
	type ListingStats,
	queryListings,
	type RegionSortField,
	type RegionStats,
	type SortField,
	sortListings,
	sortRegionStats,
	type TableRow,
	truncate,
	type ViewerFilters,
} from './pipeline/view/index.js';

// =============================================================================
// OUTPUT
// =============================================================================

export {
	buildNotionProperties,
	createNotionPage,
	type NotionConfig,
	queryExistingPages,
	updateNotionPage,
	validateDatabase,
} from './pipeline/output/index.js';

// =============================================================================
// DB
// =============================================================================

export { closeListingsDb, findListingByIdFromDb, loadListingsFile, openListingsDb, saveListingsFile, updateListingAssessment } from './db/index.js';

// =============================================================================
// UTILS
// =============================================================================

export { calculateBackoff, createRateLimiter, createResettableRateLimiter, getJitteredDelay, sleep } from './utils/http.js';
export { log } from './utils/logger.js';
