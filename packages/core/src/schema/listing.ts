/**
 * Zod schema for property listing data
 * All fields inline for easy overview
 */

import { z } from 'zod';

const StatsSummarySchema = z.object({
	min: z.number(),
	max: z.number(),
	mean: z.number(),
	median: z.number(),
	stdDev: z.number(),
});

const ScoreContextSchema = z.object({
	configHash: z.string(),
	percentiles: z.object({
		prices: StatsSummarySchema,
		trueCosts: StatsSummarySchema,
		floorAreas: StatsSummarySchema,
		stationDistances: StatsSummarySchema,
		crimeRates: StatsSummarySchema,
	}),
});

// =============================================================================
// FIELD MAPPING: PAGE_MODEL → Schema
// =============================================================================
/*
| Schema Field              | PAGE_MODEL Source                                      | Transform                    |
|---------------------------|--------------------------------------------------------|------------------------------|
| id                        | (generated)                                            | uuid                         |
| portalIds.rightmove       | propertyData.id                                        | string                       |
| url                       | (construct)                                            | https://.../${rightmoveId}   |
| location.lat              | propertyData.location.latitude                         | number                       |
| location.lng              | propertyData.location.longitude                        | number                       |
| location.pinType          | propertyData.location.pinType                          | enum                         |
| postcode                  | address.outcode + " " + address.incode                 | "PE9 2PU"                    |
| address                   | propertyData.address.displayAddress                    | string                       |
| price                     | propertyData.prices.primaryPrice                       | parse "£1,000 pcm" → 1000    |
| priceDisplay              | propertyData.prices.primaryPrice                       | string as-is                 |
| bedrooms                  | propertyData.bedrooms                                  | number                       |
| bathrooms                 | propertyData.bathrooms                                 | number                       |
| propertyType              | propertyData.propertySubType                           | string                       |
| description               | propertyData.text.description                          | strip HTML tags              |
| notes                     | (AI-populated)                                         | clean useful observations    |
| images[].remote           | propertyData.images[].url                              | full CDN URL                 |
| images[].local            | (generated)                                            | cached webp filename         |
| floorplan.remote          | propertyData.floorplans[0].url                         | first only                   |
| floorplan.local           | (generated)                                            | cached webp filename         |
| epc.remote                | propertyData.epcGraphs[0].url                          | first only                   |
| epc.local                 | (generated)                                            | cached webp filename         |
| epcRating                 | EPC API (postcode lookup)                              | A-G band                     |
| floorAreaSqm              | EPC API (postcode lookup)                              | number (sqm)                 |
| epcLodgementDate          | EPC API (postcode lookup)                              | date string                  |
| epcAddressMatch           | (generated)                                            | confidence flag              |
| nearestStations[].name    | propertyData.nearestStations[].name                    | string                       |
| nearestStations[].distance| propertyData.nearestStations[].distance                | number (miles)               |
| nearestStations[].unit    | propertyData.nearestStations[].unit                    | "miles"                      |
| listedDate                | propertyData.listingHistory.listingUpdateReason        | parse "Added on DD/MM/YYYY"  |
| gigabitAvailability       | Ofcom broadband data (SQLite lookup)                   | % premises with 1Gbps+       |
| lettings.availableDate    | propertyData.lettings.letAvailableDate                 | string                       |
| lettings.deposit          | propertyData.lettings.deposit                          | number                       |
| agent.name                | propertyData.customer.branchDisplayName                | string                       |
| agent.phone               | propertyData.contactInfo.telephoneNumbers.localNumber  | string                       |
| fetchedAt                 | (generate)                                             | ISO datetime                 |
| extractionStatus          | (generate)                                             | success/partial/failed       |
| notionPageId              | (from Notion API)                                      | page ID for sync tracking    |
*/

// =============================================================================
// LISTING SCHEMA (full inline structure)
// =============================================================================

export const ListingSchema = z.object({
	// ---------------------------------------------------------------------------
	// Identity
	// ---------------------------------------------------------------------------
	/** UUID primary key generated at ingest time */
	id: z.string().uuid(),
	/** Portal identifiers (Rightmove, Zoopla, etc.) */
	portalIds: z
		.object({
			rightmove: z.string().min(1).optional(),
			zoopla: z.string().min(1).optional(),
			onthemarket: z.string().min(1).optional(),
		})
		.default({}),
	/** UPRN (Unique Property Reference Number) when matched */
	uprn: z.string().nullable(),
	/** UPRN source/provenance */
	uprnSource: z.enum(['epc', 'os-open', 'manual']).nullable(),
	/** UPRN confidence tier */
	uprnConfidence: z.enum(['exact', 'probable', 'heuristic']).nullable(),
	/** Constructed URL: https://www.rightmove.co.uk/properties/{rightmoveId} */
	url: z.string().url(),

	// ---------------------------------------------------------------------------
	// Location
	// ---------------------------------------------------------------------------
	location: z.object({
		/** propertyData.location.latitude */
		lat: z.number().min(-90).max(90),
		/** propertyData.location.longitude */
		lng: z.number().min(-180).max(180),
		/** ACCURATE_POINT = exact building, APPROXIMATE_POINT = neighborhood */
		pinType: z.enum(['ACCURATE_POINT', 'APPROXIMATE_POINT']).nullable(),
	}),
	/** Combined from address.outcode + address.incode (e.g. "PE9 2PU") */
	postcode: z.string(),
	/** propertyData.address.displayAddress */
	address: z.string(),
	/** Rightmove search region from config (e.g. "Manchester, Greater Manchester", "Stamford, Lincolnshire").
	 * Set during batch processing from location.name. Null for manually fetched listings. */
	region: z.string().nullable().optional(),
	/** Google Maps search URL for the property address */
	googleMapsUrl: z.string().url(),
	/** Google Maps Street View URL using coordinates (may not have coverage) */
	googleMapsStreetViewUrl: z.string().url(),

	// ---------------------------------------------------------------------------
	// Area metrics (postcode/LSOA/MSOA-based)
	// ---------------------------------------------------------------------------
	area: z
		.object({
			lsoa: z.object({
				code: z.string().nullable(),
				name: z.string().nullable(),
			}),
			msoa: z.object({
				code: z.string().nullable(),
				name: z.string().nullable(),
			}),
			imd: z.object({
				rank: z.number().int().nullable(),
				decile: z.number().int().nullable(),
				score: z.number().nullable(),
			}),
			income: z.object({
				bhc: z.number().nullable(),
				ahc: z.number().nullable(),
			}),
			socialHousingPct: z.number().min(0).max(100).nullable(),
			population: z.number().int().nullable(),
			floodRisk: z.object({
				level: z.string().nullable(),
				source: z.string().nullable(),
			}),
			crime: z.object({
				count12m: z.number().int().nullable(),
				ratePer1k: z.number().nullable(),
				violent12m: z.number().int().nullable(),
				burglary12m: z.number().int().nullable(),
				robbery12m: z.number().int().nullable(),
				band: z.enum(['excellent', 'good', 'mixed', 'concerning']).nullable(),
				trend: z.enum(['improving', 'stable', 'worsening']).nullable(),
				updatedAt: z.string().datetime().nullable(),
			}),
		})
		.default({
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
		}),

	// ---------------------------------------------------------------------------
	// Property details
	// ---------------------------------------------------------------------------
	/** Parsed from propertyData.prices.primaryPrice ("£1,000 pcm" -> 1000) */
	price: z.number().nonnegative(),
	/** Original price string from Rightmove (e.g. "£1,000 pcm") */
	priceDisplay: z.string(),
	/** propertyData.bedrooms */
	bedrooms: z.number().int().nonnegative(),
	/** propertyData.bathrooms */
	bathrooms: z.number().int().min(1),
	/** propertyData.propertySubType (e.g. "Cottage", "Semi-Detached", "Terraced") */
	propertyType: z.string(),

	// ---------------------------------------------------------------------------
	// Content
	// ---------------------------------------------------------------------------
	/** Combined raw text from keyFeatures + description. Lowercase, minimal cleanup.
	 * Used as AI context for enrichment. Not human-readable. */
	description: z.string(),
	/** AI-populated notes array. Initially empty from scraper, populated by AI enrichment.
	 * Contains: property highlights, useful findings, red flags.
	 * Redundant items removed (e.g., skip "two bedrooms" when bedrooms: 2). */
	notes: z.array(z.string()),

	// ---------------------------------------------------------------------------
	// Media (remote URLs + local cached filenames)
	// ---------------------------------------------------------------------------
	/** Property images with remote URLs and optional local cached filenames */
	images: z.array(
		z.object({
			/** Full Rightmove CDN URL */
			remote: z.string().min(1),
			/** Local cached filename (e.g. "171117557-photo-a1b2c3d4.webp"), null if not cached */
			local: z.string().nullable(),
		}),
	),
	/** Floorplan with remote URL and optional local cached filename.
	 * Note: local caching is disabled (always null) due to processing issues with document-type images. */
	floorplan: z.object({
		/** Full Rightmove CDN URL, null if no floorplan available */
		remote: z.string().min(1).nullable(),
		/** Local cached filename. Always null - floorplan caching disabled due to processing issues. */
		local: z.string().nullable(),
	}),
	/** EPC graph with remote URL and optional local cached filename.
	 * Note: local caching is disabled (always null) due to processing issues with document-type images. */
	epc: z.object({
		/** Full Rightmove CDN URL, null if no EPC graph available */
		remote: z.string().min(1).nullable(),
		/** Local cached filename. Always null - EPC caching disabled due to processing issues. */
		local: z.string().nullable(),
	}),
	/** Map views for neighborhood context (satellite + street) */
	mapViews: z
		.object({
			/** Satellite/aerial imagery (no labels) */
			satellite: z.object({
				/** Mapbox Static Images API URL (without access token) */
				remote: z.string().min(1).nullable(),
				/** Local cached filename, null if not cached */
				local: z.string().nullable(),
			}),
			/** Street map with labels (POIs, streets, parks) */
			street: z.object({
				/** Mapbox Static Images API URL (without access token) */
				remote: z.string().min(1).nullable(),
				/** Local cached filename, null if not cached */
				local: z.string().nullable(),
			}),
		})
		.optional()
		.default({
			satellite: { remote: null, local: null },
			street: { remote: null, local: null },
		}),

	// ---------------------------------------------------------------------------
	// EPC API Data (authoritative)
	// ---------------------------------------------------------------------------
	/** Energy efficiency rating from EPC API (A-G) */
	epcRating: z.enum(['A', 'B', 'C', 'D', 'E', 'F', 'G']).nullable(),
	/** Floor area in square meters from EPC API */
	floorAreaSqm: z.number().positive().nullable(),
	/** EPC lodgement date (when certificate was issued) */
	epcLodgementDate: z.string().nullable(),
	/** Whether EPC record was confidently matched to this address */
	epcAddressMatch: z.boolean().nullable(),
	/** Gov.uk EPC search URL for manual lookup when API match fails */
	epcSearchUrl: z.string().url().nullable(),

	// ---------------------------------------------------------------------------
	// Transport
	// ---------------------------------------------------------------------------
	/** propertyData.nearestStations[] - pre-calculated by Rightmove */
	nearestStations: z.array(
		z.object({
			name: z.string(),
			/** Distance in miles */
			distance: z.number(),
			/** Always "miles" */
			unit: z.string(),
		}),
	),

	// ---------------------------------------------------------------------------
	// Broadband (Ofcom gigabit availability)
	// ---------------------------------------------------------------------------
	/** Gigabit availability % from Ofcom data (0-100) - percentage of premises with 1Gbps+ */
	gigabitAvailability: z.number().nonnegative().nullable(),

	// ---------------------------------------------------------------------------
	// Dates
	// ---------------------------------------------------------------------------
	/** Parsed from propertyData.listingHistory.listingUpdateReason
	 * ("Added on DD/MM/YYYY" -> "YYYY-MM-DD") */
	listedDate: z.string().nullable(),

	// ---------------------------------------------------------------------------
	// Lettings
	// ---------------------------------------------------------------------------
	lettings: z.object({
		/** propertyData.lettings.letAvailableDate ("Now" or date string) */
		availableDate: z.string().nullable(),
		/** propertyData.lettings.deposit (GBP) */
		deposit: z.number().nullable(),
	}),

	// ---------------------------------------------------------------------------
	// Agent
	// ---------------------------------------------------------------------------
	agent: z.object({
		/** propertyData.customer.branchDisplayName */
		name: z.string().nullable(),
		/** propertyData.contactInfo.telephoneNumbers.localNumber */
		phone: z.string().nullable(),
	}),

	// ---------------------------------------------------------------------------
	// AI Assessment (from Claude Code CLI)
	// ---------------------------------------------------------------------------
	/** AI assessment data - populated via `let assess <id> --json '{...}'` */
	assessment: z
		.object({
			maintenance: z.enum(['excellent', 'good', 'fair', 'poor']),
			lightAndSpace: z.string(),
			photoAnalysis: z.string(),
			tradeoffs: z.string().optional(),
			neighborhoodAnalysis: z.string().optional(),
			recommendation: z.enum(['strong-recommend', 'recommend', 'neutral', 'avoid']),
			familySuitability: z.enum(['excellent', 'good', 'fair', 'poor']),
			reasoning: z.string(),
			scoreAdjustment: z.number().optional(),
		})
		.nullable(),
	/** ISO datetime when AI assessment was performed */
	assessedAt: z.string().datetime().nullable(),
	/** Final score after AI assessment adjustment */
	assessedScore: z.number().min(0).max(100).nullable(),

	// ---------------------------------------------------------------------------
	// Scores (computed from scoring engine v2)
	// ---------------------------------------------------------------------------
	/** Computed scores from scoring engine, null before scoring.
	 * Uses variance-adaptive aggregation (geometric for consistency, arithmetic for compensation). */
	scores: z
		.object({
			/** Overall percentage score (0-100) for ranking.
			 * Derived via variance-adaptive aggregation of composites. */
			_overall: z.number(),
			/** Data completeness score (0-1). Higher = more trustworthy ranking. */
			confidence: z.number(),
			/** Affordability composite (0-100): price + floor area + running costs */
			affordability: z.number(),
			/** Location composite (0-100): station + broadband + region priority */
			location: z.number(),
			/** Liveability composite (0-100): garden + property type + heating */
			liveability: z.number(),
			/** Raw factors used in scoring for debugging/display */
			factors: z.object({
				monthlyRent: z.number(),
				pricePercentile: z.number(),
				floorAreaSqm: z.number().nullable(),
				floorAreaPercentile: z.number().nullable(),
				epcBand: z.string().nullable(),
				epcNumeric: z.number().nullable(),
				trueMonthlyCost: z.number(),
				trueCostPercentile: z.number(),
				stationMiles: z.number().nullable(),
				stationPercentile: z.number().nullable(),
				gigabitPct: z.number().nullable(),
				regionName: z.string().nullable(),
				priorityScore: z.number().nullable(),
				gardenType: z.enum(['private', 'shared', 'none']),
				heatingType: z.enum(['gas', 'electric', 'unknown']),
				petPolicy: z.enum(['yes', 'no', 'unknown']),
				propertyType: z.string().nullable(),
				bedrooms: z.number(),
				imdDecile: z.number().int().nullable(),
				crimeRatePer1k: z.number().nullable(),
				crimeRatePercentile: z.number().nullable(),
			}),
			/** Penalty multipliers applied to score */
			penalties: z.object({
				epc: z.number(),
				garden: z.number(),
				pets: z.number(),
				combined: z.number(),
			}),
			context: ScoreContextSchema,
		})
		.nullable(),

	// ---------------------------------------------------------------------------
	// Metadata
	// ---------------------------------------------------------------------------
	/** ISO datetime when this listing was fetched */
	fetchedAt: z.string().datetime(),
	/** success = all fields extracted, partial = some missing, failed = extraction error */
	extractionStatus: z.enum(['success', 'partial', 'failed']),
	/** Listing status - active (available) or inactive (let agreed, removed, or unavailable) */
	status: z.enum(['active', 'inactive']),

	// ---------------------------------------------------------------------------
	// External Sync
	// ---------------------------------------------------------------------------
	/** Notion page ID if exported to Notion database */
	notionPageId: z.string().nullable().optional(),
});

export type Listing = z.infer<typeof ListingSchema>;
