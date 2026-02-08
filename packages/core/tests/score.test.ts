import { describe, expect, test } from 'bun:test';
import { DEFAULT_SCORING_CONFIG, parseScoringConfig } from '@let/core/config';
import {
	broadbandUtility,
	buildPercentileContext,
	buildScoringContext,
	calculateAffordability,
	calculateConfidence,
	calculateLiveability,
	calculateLocation,
	calculatePenalties,
	calculatePercentile,
	describeConfidence,
	detectGardenType,
	detectHeatingType,
	detectPetPolicy,
	explainPenalties,
	extractNameFromAddress,
	extractRawFactors,
	normalizeFactors,
	recalcAssessedScores,
	scoreListingsWithConfig,
	scoreSingleListing,
	stationProximityUtility,
	varianceAdaptiveAggregate,
	weightedArithmeticMean,
	weightedGeometricMean,
} from '@let/core/pipeline/score';
import type { Listing } from '@let/core/schema';

// Helper to create a minimal listing for testing
function createListing(overrides: Partial<Listing> = {}): Listing {
	return {
		id: '123',
		portalIds: { rightmove: overrides.portalIds?.rightmove ?? overrides.id ?? '123' },
		uprn: null,
		uprnSource: null,
		uprnConfidence: null,
		url: 'https://www.rightmove.co.uk/properties/123',
		location: { lat: 53.96, lng: -1.07, pinType: null },
		postcode: 'YO24 4AB',
		address: 'Test Street, York',
		googleMapsUrl: 'https://www.google.com/maps/search/?api=1&query=Test%20Street%2C%20York%20YO24%204AB',
		googleMapsStreetViewUrl: 'https://www.google.com/maps/@?api=1&map_action=pano&viewpoint=53.96,-1.07',
		area: {
			lsoa: { code: null, name: null },
			msoa: { code: null, name: null },
			imd: { rank: null, decile: 5, score: null },
			income: { bhc: null, ahc: null },
			socialHousingPct: null,
			population: null,
			floodRisk: { level: null, source: null },
			crime: {
				count12m: null,
				ratePer1k: 25,
				violent12m: null,
				burglary12m: null,
				robbery12m: null,
				band: null,
				trend: null,
				updatedAt: null,
			},
		},
		price: 1200,
		priceDisplay: '£1,200 pcm',
		bedrooms: 2,
		bathrooms: 1,
		propertyType: 'Terraced',
		description: '',
		notes: [],
		images: [],
		floorplan: { remote: null, local: null },
		epc: { remote: null, local: null },
		mapViews: { satellite: { remote: null, local: null }, street: { remote: null, local: null } },
		epcRating: null,
		floorAreaSqm: null,
		epcLodgementDate: null,
		epcAddressMatch: null,
		epcSearchUrl: 'https://find-energy-certificate.service.gov.uk/find-a-certificate/search-by-postcode?postcode=YO24%204AB',
		nearestStations: [],
		gigabitAvailability: null,
		listedDate: null,
		lettings: { availableDate: null, deposit: null },
		agent: { name: null, phone: null },
		assessment: null,
		assessedAt: null,
		assessedScore: null,
		scores: null,
		fetchedAt: new Date().toISOString(),
		extractionStatus: 'success',
		status: 'active',
		...overrides,
	};
}

// =============================================================================
// DETECTION FUNCTIONS
// =============================================================================

describe('detectGardenType', () => {
	test('detects private garden from description', () => {
		expect(detectGardenType(createListing({ description: 'Private rear garden' }))).toBe('private');
		expect(detectGardenType(createListing({ description: 'Enclosed garden to rear' }))).toBe('private');
		expect(detectGardenType(createListing({ description: 'South-facing garden' }))).toBe('private');
	});

	test('detects private garden from notes', () => {
		expect(detectGardenType(createListing({ notes: ['Private Garden'] }))).toBe('private');
		expect(detectGardenType(createListing({ notes: ['Rear Garden', 'Gas Heating'] }))).toBe('private');
	});

	test('detects shared garden', () => {
		expect(detectGardenType(createListing({ description: 'Shared garden with other residents' }))).toBe('shared');
		expect(detectGardenType(createListing({ description: 'Communal garden area' }))).toBe('shared');
	});

	test('detects no garden', () => {
		expect(detectGardenType(createListing({ description: 'No garden' }))).toBe('none');
		expect(detectGardenType(createListing({ description: 'City centre flat' }))).toBe('none');
	});

	test('assumes private for generic garden mention', () => {
		expect(detectGardenType(createListing({ description: 'Property has a garden' }))).toBe('private');
	});
});

describe('detectHeatingType', () => {
	test('detects gas heating', () => {
		expect(detectHeatingType(createListing({ description: 'Gas central heating' }))).toBe('gas');
		expect(detectHeatingType(createListing({ notes: ['Gas Central Heating'] }))).toBe('gas');
		expect(detectHeatingType(createListing({ description: 'Gas CH throughout' }))).toBe('gas');
	});

	test('detects electric heating', () => {
		expect(detectHeatingType(createListing({ description: 'Electric storage heaters' }))).toBe('electric');
		expect(detectHeatingType(createListing({ description: 'Electric heating' }))).toBe('electric');
	});

	test('returns unknown when heating not mentioned', () => {
		expect(detectHeatingType(createListing({ description: 'Nice property' }))).toBe('unknown');
	});
});

describe('detectPetPolicy', () => {
	test('detects pet-friendly', () => {
		expect(detectPetPolicy(createListing({ description: 'Pets considered' }))).toBe('yes');
		expect(detectPetPolicy(createListing({ description: 'Pets allowed' }))).toBe('yes');
		expect(detectPetPolicy(createListing({ description: 'Pet-friendly property' }))).toBe('yes');
	});

	test('detects no pets', () => {
		expect(detectPetPolicy(createListing({ description: 'No pets' }))).toBe('no');
		expect(detectPetPolicy(createListing({ description: 'Pets not allowed' }))).toBe('no');
	});

	test('returns unknown when not specified', () => {
		expect(detectPetPolicy(createListing({ description: 'Nice property' }))).toBe('unknown');
	});
});

describe('extractNameFromAddress', () => {
	test('extracts region from address string', () => {
		expect(extractNameFromAddress('High Street, York')).toBe('York');
		expect(extractNameFromAddress('Hillside, Sheffield, S10')).toBe('Sheffield');
		expect(extractNameFromAddress('Jesmond, Newcastle upon Tyne')).toBe('Newcastle');
	});

	test('matches provided region list case-insensitively', () => {
		const regions = ['York', 'Sheffield', 'Testville'];
		expect(extractNameFromAddress('Hillside, SHEFFIELD, S10', regions)).toBe('Sheffield');
		expect(extractNameFromAddress('High Street, york', regions)).toBe('York');
		expect(extractNameFromAddress('Market Road, Testville', regions)).toBe('Testville');
	});

	test('returns null for unknown region', () => {
		expect(extractNameFromAddress('Random Place, Unknown Town')).toBeNull();
	});

	test('handles region in various positions', () => {
		expect(extractNameFromAddress('123 Main St, Manchester')).toBe('Manchester');
		expect(extractNameFromAddress('Leeds City Centre')).toBe('Leeds');
	});

	test('does not match region substring in county names', () => {
		// "York" should not match "Yorkshire" - should find "Sheffield" instead
		expect(extractNameFromAddress('Rydal Crescent, Sheffield, South Yorkshire, S8')).toBe('Sheffield');
		expect(extractNameFromAddress('Some Street, South Yorkshire')).toBeNull();
		expect(extractNameFromAddress('West Yorkshire Village')).toBeNull();
	});
});

// =============================================================================
// PERCENTILE CALCULATIONS
// =============================================================================

describe('calculatePercentile', () => {
	test('calculates correct percentile', () => {
		const sortedArray = [100, 200, 300, 400, 500];
		// Percentile rank: position / n * 100
		expect(calculatePercentile(100, sortedArray)).toBe(0); // lowest value = 0th percentile
		expect(calculatePercentile(500, sortedArray)).toBe(80); // highest value = 80th percentile (4/5 * 100)
		expect(calculatePercentile(300, sortedArray)).toBe(40); // 2/5 * 100 = 40th percentile
	});

	test('inverts percentile when requested', () => {
		const sortedArray = [100, 200, 300, 400, 500];
		// For price: lower is better, so invert
		expect(calculatePercentile(100, sortedArray, true)).toBe(100); // lowest price = 100th percentile (best)
		expect(calculatePercentile(500, sortedArray, true)).toBe(20); // highest price = 20th percentile (100 - 80)
	});

	test('handles empty array', () => {
		expect(calculatePercentile(100, [])).toBe(50); // fallback to middle
	});

	test('handles single element array', () => {
		expect(calculatePercentile(100, [100])).toBe(50);
	});

	test('handles two-element array with rank-based percentiles', () => {
		expect(calculatePercentile(100, [100, 200])).toBe(0);
		expect(calculatePercentile(200, [100, 200])).toBe(100);
		expect(calculatePercentile(100, [100, 200], true)).toBe(100);
		expect(calculatePercentile(200, [100, 200], true)).toBe(0);
	});
});

describe('buildPercentileContext', () => {
	test('builds percentile arrays from listings', () => {
		const listings = [createListing({ price: 1000 }), createListing({ price: 1200 }), createListing({ price: 800 })];
		const config = parseScoringConfig({ scoring: DEFAULT_SCORING_CONFIG });
		const context = buildPercentileContext(listings, config);

		expect(context.prices).toHaveLength(3);
		expect(context.prices).toEqual([800, 1000, 1200]); // sorted
		expect(context.trueCosts).toHaveLength(3); // prices + default heating costs
	});

	test('handles listings with floor area', () => {
		const listings = [createListing({ floorAreaSqm: 80 }), createListing({ floorAreaSqm: 60 }), createListing({ floorAreaSqm: null })];
		const config = parseScoringConfig({ scoring: DEFAULT_SCORING_CONFIG });
		const context = buildPercentileContext(listings, config);

		expect(context.floorAreas).toHaveLength(2); // only non-null values
		expect(context.floorAreas).toEqual([60, 80]); // sorted
	});
});

// =============================================================================
// COMPOSITE CALCULATIONS
// =============================================================================

describe('calculateAffordability', () => {
	test('higher percentile means better affordability', () => {
		const config = parseScoringConfig({ scoring: DEFAULT_SCORING_CONFIG });

		// Cheap property with good EPC
		const cheapFactors = {
			monthlyRent: 800,
			pricePercentile: 90, // 90th percentile (cheaper than 90%)
			floorAreaSqm: 80,
			floorAreaPercentile: 70,
			epcBand: 'B',
			epcNumeric: 85,
			trueMonthlyCost: 845,
			trueCostPercentile: 85,
			stationMiles: null,
			stationPercentile: null,
			gigabitPct: null,
			regionName: 'York',
			priorityScore: 95,
			gardenType: 'private' as const,
			heatingType: 'gas' as const,
			petPolicy: 'unknown' as const,
			propertyType: 'terraced',
			bedrooms: 2,
			imdDecile: null,
			crimeRatePer1k: null,
			crimeRatePercentile: null,
		};

		// Expensive property with poor EPC
		const expensiveFactors = {
			...cheapFactors,
			monthlyRent: 1500,
			pricePercentile: 20,
			trueMonthlyCost: 1700,
			trueCostPercentile: 15,
			epcBand: 'E',
			epcNumeric: 40,
		};

		const cheapScore = calculateAffordability(cheapFactors, config.affordability);
		const expensiveScore = calculateAffordability(expensiveFactors, config.affordability);

		expect(cheapScore).toBeGreaterThan(expensiveScore);
		expect(cheapScore).toBeGreaterThan(0.5); // Should be decent
		expect(expensiveScore).toBeLessThan(0.5); // Should be poor
	});

	test('epc score does not affect affordability when epc weight is zero', () => {
		const config = parseScoringConfig({
			scoring: {
				...DEFAULT_SCORING_CONFIG,
				affordability: {
					...DEFAULT_SCORING_CONFIG.affordability,
					priceWeight: 1.0,
					epcWeight: 0.0,
				},
			},
		});

		const baseFactors = {
			monthlyRent: 1000,
			pricePercentile: 50,
			floorAreaSqm: 80,
			floorAreaPercentile: 50,
			epcBand: 'C',
			epcNumeric: 70,
			trueMonthlyCost: 1070,
			trueCostPercentile: 50,
			stationMiles: null,
			stationPercentile: null,
			gigabitPct: null,
			regionName: null,
			priorityScore: null,
			gardenType: 'private' as const,
			heatingType: 'gas' as const,
			petPolicy: 'unknown' as const,
			propertyType: 'terraced',
			bedrooms: 2,
			imdDecile: null,
			crimeRatePer1k: null,
			crimeRatePercentile: null,
		};

		const lowEpc = calculateAffordability({ ...baseFactors, epcBand: 'G', epcNumeric: 10 }, config.affordability);
		const highEpc = calculateAffordability({ ...baseFactors, epcBand: 'A', epcNumeric: 100 }, config.affordability);

		expect(lowEpc).toBeCloseTo(highEpc, 6);
	});
});

describe('calculateLocation', () => {
	test('closer station means better location', () => {
		const config = parseScoringConfig({ scoring: DEFAULT_SCORING_CONFIG });

		const baseFactors = {
			monthlyRent: 1000,
			pricePercentile: 50,
			floorAreaSqm: 70,
			floorAreaPercentile: 50,
			epcBand: 'C',
			epcNumeric: 70,
			trueMonthlyCost: 1070,
			trueCostPercentile: 50,
			gigabitPct: 80,
			regionName: 'York',
			priorityScore: 95,
			gardenType: 'private' as const,
			heatingType: 'gas' as const,
			petPolicy: 'unknown' as const,
			propertyType: 'terraced',
			bedrooms: 2,
			imdDecile: null,
			crimeRatePer1k: null,
			crimeRatePercentile: null,
		};

		const closeStation = calculateLocation({ ...baseFactors, stationMiles: 0.3, stationPercentile: 90 }, config.location);
		const farStation = calculateLocation({ ...baseFactors, stationMiles: 2.0, stationPercentile: 20 }, config.location);

		expect(closeStation).toBeGreaterThan(farStation);
	});

	test('high region priority improves location score', () => {
		const config = parseScoringConfig({ scoring: DEFAULT_SCORING_CONFIG });

		const baseFactors = {
			monthlyRent: 1000,
			pricePercentile: 50,
			floorAreaSqm: 70,
			floorAreaPercentile: 50,
			epcBand: 'C',
			epcNumeric: 70,
			trueMonthlyCost: 1070,
			trueCostPercentile: 50,
			stationMiles: 0.5,
			stationPercentile: 80,
			gigabitPct: 80,
			gardenType: 'private' as const,
			heatingType: 'gas' as const,
			petPolicy: 'unknown' as const,
			propertyType: 'terraced',
			bedrooms: 2,
			imdDecile: null,
			crimeRatePer1k: null,
			crimeRatePercentile: null,
		};

		const highPriority = calculateLocation({ ...baseFactors, regionName: 'York', priorityScore: 95 }, config.location);
		const lowPriority = calculateLocation({ ...baseFactors, regionName: 'Manchester', priorityScore: 65 }, config.location);

		expect(highPriority).toBeGreaterThan(lowPriority);
	});

	test('missing imd and crime redistributes weights', () => {
		const config = parseScoringConfig({ scoring: DEFAULT_SCORING_CONFIG });

		const baseFactors = {
			monthlyRent: 1000,
			pricePercentile: 50,
			floorAreaSqm: 70,
			floorAreaPercentile: 50,
			epcBand: 'C',
			epcNumeric: 70,
			trueMonthlyCost: 1070,
			trueCostPercentile: 50,
			stationMiles: 0.8,
			stationPercentile: 70,
			gigabitPct: 80,
			regionName: 'York',
			priorityScore: 95,
			gardenType: 'private' as const,
			heatingType: 'gas' as const,
			petPolicy: 'unknown' as const,
			propertyType: 'terraced',
			bedrooms: 2,
			imdDecile: null,
			crimeRatePer1k: null,
			crimeRatePercentile: null,
		};

		const withConfiguredWeights = calculateLocation(baseFactors, config.location);
		const zeroWeights = { ...config.location, imdWeight: 0, crimeWeight: 0 };
		const withZeroWeights = calculateLocation(baseFactors, zeroWeights);

		expect(withConfiguredWeights).toBeCloseTo(withZeroWeights, 6);
	});

	test('missing region priority redistributes weights', () => {
		const config = parseScoringConfig({ scoring: DEFAULT_SCORING_CONFIG });

		const baseFactors = {
			monthlyRent: 1000,
			pricePercentile: 50,
			floorAreaSqm: 70,
			floorAreaPercentile: 50,
			epcBand: 'C',
			epcNumeric: 70,
			trueMonthlyCost: 1070,
			trueCostPercentile: 50,
			stationMiles: 0.4,
			stationPercentile: 85,
			gigabitPct: 90,
			regionName: null,
			priorityScore: null,
			gardenType: 'private' as const,
			heatingType: 'gas' as const,
			petPolicy: 'unknown' as const,
			propertyType: 'terraced',
			bedrooms: 2,
			imdDecile: 10,
			crimeRatePer1k: 5,
			crimeRatePercentile: 95,
		};

		const expected = weightedArithmeticMean([
			[stationProximityUtility(baseFactors.stationMiles), config.location.stationWeight],
			[broadbandUtility(baseFactors.gigabitPct), config.location.broadbandWeight],
			[1, config.location.imdWeight],
			[baseFactors.crimeRatePercentile / 100, config.location.crimeWeight],
		]);

		const score = calculateLocation(baseFactors, config.location);
		expect(score).toBeCloseTo(expected, 6);
	});
});

describe('calculateLiveability', () => {
	test('private garden scores higher than none', () => {
		const config = parseScoringConfig({ scoring: DEFAULT_SCORING_CONFIG });

		const baseFactors = {
			monthlyRent: 1000,
			pricePercentile: 50,
			floorAreaSqm: 70,
			floorAreaPercentile: 50,
			epcBand: 'C',
			epcNumeric: 70,
			trueMonthlyCost: 1070,
			trueCostPercentile: 50,
			stationMiles: 0.5,
			stationPercentile: 80,
			gigabitPct: 80,
			regionName: 'York',
			priorityScore: 95,
			heatingType: 'gas' as const,
			petPolicy: 'unknown' as const,
			propertyType: 'terraced',
			bedrooms: 2,
			imdDecile: null,
			crimeRatePer1k: null,
			crimeRatePercentile: null,
		};

		const withGarden = calculateLiveability({ ...baseFactors, gardenType: 'private' }, config.liveability);
		const noGarden = calculateLiveability({ ...baseFactors, gardenType: 'none' }, config.liveability);

		expect(withGarden).toBeGreaterThan(noGarden);
	});

	test('gas heating scores higher than electric', () => {
		const config = parseScoringConfig({ scoring: DEFAULT_SCORING_CONFIG });

		const baseFactors = {
			monthlyRent: 1000,
			pricePercentile: 50,
			floorAreaSqm: 70,
			floorAreaPercentile: 50,
			epcBand: 'C',
			epcNumeric: 70,
			trueMonthlyCost: 1070,
			trueCostPercentile: 50,
			stationMiles: 0.5,
			stationPercentile: 80,
			gigabitPct: 80,
			regionName: 'York',
			priorityScore: 95,
			gardenType: 'private' as const,
			petPolicy: 'unknown' as const,
			propertyType: 'terraced',
			bedrooms: 2,
			imdDecile: null,
			crimeRatePer1k: null,
			crimeRatePercentile: null,
		};

		const gasHeating = calculateLiveability({ ...baseFactors, heatingType: 'gas' }, config.liveability);
		const electricHeating = calculateLiveability({ ...baseFactors, heatingType: 'electric' }, config.liveability);

		expect(gasHeating).toBeGreaterThan(electricHeating);
	});
});

// =============================================================================
// PENALTIES
// =============================================================================

describe('calculatePenalties', () => {
	const config = parseScoringConfig({ scoring: DEFAULT_SCORING_CONFIG });

	const baseFactors = {
		monthlyRent: 1000,
		pricePercentile: 50,
		floorAreaSqm: 70,
		floorAreaPercentile: 50,
		epcBand: 'C',
		epcNumeric: 70,
		trueMonthlyCost: 1070,
		trueCostPercentile: 50,
		stationMiles: 0.5,
		stationPercentile: 80,
		gigabitPct: 80,
		regionName: 'York',
		priorityScore: 95,
		gardenType: 'private' as const,
		heatingType: 'gas' as const,
		petPolicy: 'unknown' as const,
		propertyType: 'terraced',
		bedrooms: 2,
		imdDecile: 5,
		crimeRatePer1k: 25,
		crimeRatePercentile: null,
	};

	test('no penalties for good property', () => {
		const penalties = calculatePenalties(baseFactors, config.penalties);
		expect(penalties.combined).toBe(1.0);
		expect(penalties.epc).toBe(1.0);
		expect(penalties.garden).toBe(1.0);
		expect(penalties.pets).toBe(1.0);
	});

	test('EPC F gets severe penalty', () => {
		const penalties = calculatePenalties({ ...baseFactors, epcBand: 'F' }, { ...config.penalties, epcF: 0.3 });
		expect(penalties.epc).toBe(0.3);
		expect(penalties.combined).toBeLessThan(0.5);
	});

	test('EPC G gets maximum penalty', () => {
		const penalties = calculatePenalties({ ...baseFactors, epcBand: 'G' }, { ...config.penalties, epcG: 0.1 });
		expect(penalties.epc).toBe(0.1);
		expect(penalties.combined).toBeLessThan(0.2);
	});

	test('no garden gets penalty when gardenRequired', () => {
		// Create config with gardenRequired: true
		const penaltyConfigWithGarden = { ...config.penalties, gardenRequired: true };
		const penalties = calculatePenalties({ ...baseFactors, gardenType: 'none' }, penaltyConfigWithGarden);
		expect(penalties.garden).toBe(0.5);
	});

	test('no garden gets no penalty when gardenRequired is false', () => {
		// Default config has gardenRequired: false
		const penalties = calculatePenalties({ ...baseFactors, gardenType: 'none' }, config.penalties);
		expect(penalties.garden).toBe(1.0);
	});

	test('no pets gets penalty', () => {
		const penalties = calculatePenalties({ ...baseFactors, petPolicy: 'no' }, config.penalties);
		expect(penalties.pets).toBe(0.4);
	});

	test('penalties multiply together', () => {
		const penaltyConfigWithGarden = { ...config.penalties, gardenRequired: true, epcF: 0.3 };
		const penalties = calculatePenalties({ ...baseFactors, epcBand: 'F', gardenType: 'none', petPolicy: 'no' }, penaltyConfigWithGarden);
		// 0.3 (EPC F) * 0.5 (no garden) * 0.4 (no pets) = 0.06
		expect(penalties.combined).toBeCloseTo(0.06, 2);
	});

	test('missing data applies penalty multiplier', () => {
		const penaltyConfig = { ...config.penalties, missingDataPenalty: 0.9 };
		const penalties = calculatePenalties(
			{
				...baseFactors,
				epcBand: null,
				stationMiles: null,
				gigabitPct: null,
				priorityScore: null,
				imdDecile: null,
				crimeRatePer1k: null,
			},
			penaltyConfig,
		);
		expect(penalties.combined).toBeCloseTo(0.9 ** 6, 4);
	});
});

describe('explainPenalties', () => {
	test('explains EPC penalty', () => {
		const penalties = {
			epc: 0.3,
			garden: 1.0,
			pets: 1.0,
			combined: 0.3,
		};
		const explanations = explainPenalties(penalties);
		expect(explanations.length).toBe(1);
		expect(explanations[0]).toContain('EPC F');
	});

	test('explains multiple penalties', () => {
		const penalties = {
			epc: 0.1,
			garden: 0.5,
			pets: 0.4,
			combined: 0.02,
		};
		const explanations = explainPenalties(penalties);
		expect(explanations.length).toBe(3);
	});

	test('returns empty array for no penalties', () => {
		const penalties = {
			epc: 1.0,
			garden: 1.0,
			pets: 1.0,
			combined: 1.0,
		};
		const explanations = explainPenalties(penalties);
		expect(explanations.length).toBe(0);
	});
});

// =============================================================================
// CONFIDENCE
// =============================================================================

describe('calculateConfidence', () => {
	const config = parseScoringConfig({ scoring: DEFAULT_SCORING_CONFIG });

	test('high confidence with complete data', () => {
		const factors = {
			monthlyRent: 1000,
			pricePercentile: 50,
			floorAreaSqm: 70,
			floorAreaPercentile: 50,
			epcBand: 'C',
			epcNumeric: 70,
			trueMonthlyCost: 1070,
			trueCostPercentile: 50,
			stationMiles: 0.5,
			stationPercentile: 80,
			gigabitPct: 80,
			regionName: 'York',
			priorityScore: 95,
			gardenType: 'private' as const,
			heatingType: 'gas' as const,
			petPolicy: 'yes' as const,
			propertyType: 'terraced',
			bedrooms: 2,
			imdDecile: 7,
			crimeRatePer1k: 50,
			crimeRatePercentile: 80,
		};

		const confidence = calculateConfidence(factors, config);
		expect(confidence.score).toBeGreaterThan(0.9);
	});

	test('lower confidence with missing data', () => {
		const incompleteFactors = {
			monthlyRent: 1000,
			pricePercentile: 50,
			floorAreaSqm: null,
			floorAreaPercentile: null,
			epcBand: null,
			epcNumeric: null,
			trueMonthlyCost: 1100,
			trueCostPercentile: 50,
			stationMiles: null,
			stationPercentile: null,
			gigabitPct: null,
			regionName: null,
			priorityScore: null,
			gardenType: 'none' as const,
			heatingType: 'unknown' as const,
			petPolicy: 'unknown' as const,
			propertyType: null,
			bedrooms: 2,
			imdDecile: null,
			crimeRatePer1k: null,
			crimeRatePercentile: null,
		};

		const confidence = calculateConfidence(incompleteFactors, config);
		expect(confidence.score).toBeLessThan(0.7);
		expect(confidence.missingFactors.length).toBeGreaterThan(0);
	});
});

describe('describeConfidence', () => {
	test('describes high confidence', () => {
		const result = describeConfidence({
			score: 0.95,
			availableFactors: ['price', 'floorArea', 'epc', 'station'],
			missingFactors: [],
			quality: 'high',
		});
		expect(result).toContain('High');
	});

	test('describes low confidence', () => {
		const result = describeConfidence({
			score: 0.45,
			availableFactors: ['price'],
			missingFactors: ['floorArea', 'epc', 'station'],
			quality: 'low',
		});
		expect(result).toContain('Low');
	});
});

// =============================================================================
// FULL SCORING FLOW
// =============================================================================

describe('scoreSingleListing', () => {
	test('scores a well-matched listing highly', () => {
		const listings = [
			createListing({
				id: '1',
				address: 'Maple Avenue, York',
				price: 850,
				description: 'Private garden. Gas central heating. Pets considered.',
				notes: ['Private Garden', 'Gas Central Heating'],
				nearestStations: [{ name: 'York', distance: 0.5, unit: 'miles' }],
				gigabitAvailability: 95,
				epcRating: 'B',
				floorAreaSqm: 85,
			}),
			createListing({
				id: '2',
				address: 'Some Street, Manchester',
				price: 1200,
				description: 'City centre flat. No garden.',
				nearestStations: [{ name: 'Manchester Piccadilly', distance: 1.2, unit: 'miles' }],
				gigabitAvailability: 70,
				epcRating: 'D',
				floorAreaSqm: 55,
			}),
		];

		const config = parseScoringConfig({ scoring: DEFAULT_SCORING_CONFIG });
		const context = buildScoringContext(listings, config);

		const firstListing = listings[0];
		if (!firstListing) throw new Error('Expected firstListing');
		const scored = scoreSingleListing(firstListing, context);

		// Good property should score well
		expect(scored.scores._overall).toBeGreaterThan(50);
		expect(scored.scores.confidence).toBeGreaterThan(0.8);
		expect(scored.scores.factors.gardenType).toBe('private');
		expect(scored.scores.factors.heatingType).toBe('gas');
		expect(scored.scores.factors.epcBand).toBe('B');
		expect(scored.scores.context.configHash).toHaveLength(64);
		expect(scored.scores.context.percentiles.prices.mean).toBeGreaterThan(0);
	});

	test('scores a poor property lowly', () => {
		const listings = [
			createListing({
				id: '1',
				price: 700,
				description: 'No garden. Electric heating. No pets.',
				nearestStations: [],
				epcRating: 'F',
				floorAreaSqm: 45,
			}),
		];

		// Use config with gardenRequired to apply garden penalty
		const configWithGarden = {
			...DEFAULT_SCORING_CONFIG,
			penalties: { ...DEFAULT_SCORING_CONFIG.penalties, gardenRequired: true },
		};
		const config = parseScoringConfig({ scoring: configWithGarden });
		const context = buildScoringContext(listings, config);

		const firstListing = listings[0];
		if (!firstListing) throw new Error('Expected firstListing');
		const scored = scoreSingleListing(firstListing, context);

		// Should have multiple penalties
		expect(scored.scores.penalties.epc).toBeLessThan(1.0); // EPC F penalty
		expect(scored.scores.penalties.garden).toBeLessThan(1.0); // No garden penalty (gardenRequired: true)
		expect(scored.scores.penalties.pets).toBeLessThan(1.0); // No pets penalty
		expect(scored.scores._overall).toBeLessThan(30); // Overall should be low
	});
});

describe('scoreListingsWithConfig', () => {
	test('returns listings sorted by score descending', () => {
		const listings = [
			createListing({ id: '1', price: 1500, description: 'No garden. EPC F.' }),
			createListing({
				id: '2',
				price: 900,
				description: 'Private garden. Gas heating.',
				epcRating: 'B',
				floorAreaSqm: 80,
			}),
			createListing({
				id: '3',
				price: 1100,
				description: 'Shared garden.',
				epcRating: 'C',
			}),
		];

		const rawConfig = { scoring: DEFAULT_SCORING_CONFIG };
		const scored = scoreListingsWithConfig(listings, rawConfig);

		expect(scored).toHaveLength(3);
		// Sorted by score descending
		const [first, second, third] = scored;
		if (!first || !second || !third) throw new Error('Expected 3 scored listings');
		expect(first.scores._overall).toBeGreaterThanOrEqual(second.scores._overall);
		expect(second.scores._overall).toBeGreaterThanOrEqual(third.scores._overall);
	});

	test('golden ranking order stays stable', () => {
		const listings = [
			createListing({
				id: 'A',
				address: 'Best Street, York',
				price: 800,
				description: 'Private garden. Gas central heating. Pets considered.',
				nearestStations: [{ name: 'York', distance: 0.3, unit: 'miles' }],
				gigabitAvailability: 95,
				epcRating: 'B',
				floorAreaSqm: 90,
				propertyType: 'Terraced',
				bedrooms: 3,
			}),
			createListing({
				id: 'B',
				address: 'Good Road, York',
				price: 900,
				description: 'Shared garden. Gas heating. Pets considered.',
				nearestStations: [{ name: 'York', distance: 0.6, unit: 'miles' }],
				gigabitAvailability: 85,
				epcRating: 'C',
				floorAreaSqm: 80,
				propertyType: 'Terraced',
				bedrooms: 2,
			}),
			createListing({
				id: 'C',
				address: 'High Street, York',
				price: 1200,
				description: 'Private garden. Gas central heating. Pets considered.',
				nearestStations: [{ name: 'York', distance: 0.4, unit: 'miles' }],
				gigabitAvailability: 90,
				epcRating: 'B',
				floorAreaSqm: 95,
				propertyType: 'Detached',
				bedrooms: 3,
			}),
			createListing({
				id: 'D',
				address: 'Market Street, Manchester',
				price: 1000,
				description: 'Shared garden. Electric heating.',
				nearestStations: [{ name: 'Manchester Piccadilly', distance: 1.2, unit: 'miles' }],
				gigabitAvailability: null,
				epcRating: null,
				floorAreaSqm: 70,
				propertyType: 'Flat',
				bedrooms: 2,
			}),
			createListing({
				id: 'E',
				address: 'Town Road, Manchester',
				price: 950,
				description: 'No garden. Electric heating. No pets.',
				nearestStations: [{ name: 'Manchester Piccadilly', distance: 1.5, unit: 'miles' }],
				gigabitAvailability: 60,
				epcRating: 'E',
				floorAreaSqm: 65,
				propertyType: 'Flat',
				bedrooms: 2,
			}),
			createListing({
				id: 'F',
				address: 'Far Road, Manchester',
				price: 1600,
				description: 'No garden. Electric heating. No pets.',
				nearestStations: [{ name: 'Manchester Piccadilly', distance: 3.0, unit: 'miles' }],
				gigabitAvailability: 30,
				epcRating: 'F',
				floorAreaSqm: 50,
				propertyType: 'Flat',
				bedrooms: 1,
			}),
		];

		const scored = scoreListingsWithConfig(listings, { scoring: DEFAULT_SCORING_CONFIG });
		const order = scored.map((listing) => listing.id);

		expect(order).toEqual(['A', 'B', 'C', 'D', 'E', 'F']);
	});

	test('handles empty listings array', () => {
		const rawConfig = { scoring: DEFAULT_SCORING_CONFIG };
		const scored = scoreListingsWithConfig([], rawConfig);
		expect(scored).toHaveLength(0);
	});
});

describe('extractRawFactors', () => {
	test('extracts all factors from listing', () => {
		const listing = createListing({
			price: 1000,
			epcRating: 'B',
			floorAreaSqm: 75,
			nearestStations: [{ name: 'York', distance: 0.8, unit: 'miles' }],
			gigabitAvailability: 90,
			description: 'Private garden. Gas central heating.',
			propertyType: 'Terraced',
			bedrooms: 3,
		});

		const factors = extractRawFactors(listing);

		expect(factors.monthlyRent).toBe(1000);
		expect(factors.floorAreaSqm).toBe(75);
		expect(factors.epcBand).toBe('B');
		expect(factors.stationMiles).toBe(0.8);
		expect(factors.gigabitPct).toBe(90);
		expect(factors.gardenType).toBe('private');
		expect(factors.heatingType).toBe('gas');
		expect(factors.propertyType).toBe('terraced');
		expect(factors.bedrooms).toBe(3);
	});

	test('persists region to listing.region when null', () => {
		const listing = createListing({
			region: null,
			address: '42 Test Street, Sheffield',
		});

		extractRawFactors(listing, ['Sheffield']);

		expect(listing.region).toBe('Sheffield');
	});

	test('does not overwrite existing region', () => {
		const listing = createListing({
			region: 'Manchester',
			address: '42 Test Street, Sheffield',
		});

		extractRawFactors(listing, ['Sheffield', 'Manchester']);

		expect(listing.region).toBe('Manchester');
	});

	test('handles missing data gracefully', () => {
		const listing = createListing({
			price: 1000,
			epcRating: null,
			floorAreaSqm: null,
			nearestStations: [],
			gigabitAvailability: null,
		});

		const factors = extractRawFactors(listing);

		expect(factors.floorAreaSqm).toBeNull();
		expect(factors.epcBand).toBeNull();
		expect(factors.stationMiles).toBeNull();
		expect(factors.gigabitPct).toBeNull();
	});
});

describe('normalizeFactors', () => {
	test('normalizes factors with percentile context', () => {
		const listings = [createListing({ price: 800 }), createListing({ price: 1000 }), createListing({ price: 1200 })];
		const config = parseScoringConfig({ scoring: DEFAULT_SCORING_CONFIG });
		const percentiles = buildPercentileContext(listings, config);

		const raw = extractRawFactors(createListing({ price: 800, epcRating: 'B' }));
		const normalized = normalizeFactors(raw, percentiles, config);

		// Cheapest price should have high percentile
		expect(normalized.pricePercentile).toBeGreaterThan(70);
		expect(normalized.epcNumeric).toBe(85); // B = 85
	});

	test('matches region priority case-insensitively', () => {
		const config = parseScoringConfig({
			scoring: {
				...DEFAULT_SCORING_CONFIG,
				regionPriority: {
					York: 95,
				},
			},
		});
		const percentiles = buildPercentileContext([createListing({ price: 800 })], config);

		const raw = extractRawFactors(createListing({ region: 'york', address: 'Test Street, york' }), Object.keys(config.regionPriority));
		const normalized = normalizeFactors(raw, percentiles, config);

		expect(normalized.regionName).toBe('York');
		expect(normalized.priorityScore).toBe(95);
	});
});

// =============================================================================
// VARIANCE-ADAPTIVE AGGREGATION
// =============================================================================

describe('weightedGeometricMean', () => {
	test('calculates geometric mean with equal weights', () => {
		// Geometric mean of (0.5, 0.5, 0.5) = 0.5
		const result = weightedGeometricMean([
			[0.5, 1],
			[0.5, 1],
			[0.5, 1],
		]);
		expect(result).toBeCloseTo(0.5, 5);
	});

	test('penalizes properties with one weak factor heavily', () => {
		// One weakness (0.3) with two strengths (0.9, 0.9)
		// Geometric mean = (0.3 * 0.9 * 0.9)^(1/3) ≈ 0.59
		const result = weightedGeometricMean([
			[0.3, 1],
			[0.9, 1],
			[0.9, 1],
		]);
		expect(result).toBeLessThan(0.65);
		expect(result).toBeGreaterThan(0.55);
	});

	test('handles zero values', () => {
		const result = weightedGeometricMean([
			[0.0, 1],
			[0.9, 1],
			[0.9, 1],
		]);
		// Should return near-zero for zero input
		expect(result).toBeLessThan(0.1);
	});
});

describe('weightedArithmeticMean', () => {
	test('calculates arithmetic mean with equal weights', () => {
		// Arithmetic mean of (0.5, 0.5, 0.5) = 0.5
		const result = weightedArithmeticMean([
			[0.5, 1],
			[0.5, 1],
			[0.5, 1],
		]);
		expect(result).toBeCloseTo(0.5, 5);
	});

	test('allows compensation for weak factors', () => {
		// One weakness (0.3) with two strengths (0.9, 0.9)
		// Arithmetic mean = (0.3 + 0.9 + 0.9) / 3 = 0.7
		const result = weightedArithmeticMean([
			[0.3, 1],
			[0.9, 1],
			[0.9, 1],
		]);
		expect(result).toBeCloseTo(0.7, 5);
	});
});

describe('varianceAdaptiveAggregate', () => {
	test('consistent mediocrity produces geometric-like result', () => {
		// CV = 0 for identical values -> should be close to geometric mean
		const result = varianceAdaptiveAggregate([
			[0.5, 1],
			[0.5, 1],
			[0.5, 1],
		]);
		expect(result).toBeCloseTo(0.5, 2);
	});

	test('high variance allows compensation for weak factors', () => {
		// One weakness (0.3), two strengths (0.9, 0.9)
		// High CV -> shift toward arithmetic mean -> allow compensation
		const variance = varianceAdaptiveAggregate([
			[0.3, 1],
			[0.9, 1],
			[0.9, 1],
		]);
		const geometric = weightedGeometricMean([
			[0.3, 1],
			[0.9, 1],
			[0.9, 1],
		]);
		const arithmetic = weightedArithmeticMean([
			[0.3, 1],
			[0.9, 1],
			[0.9, 1],
		]);

		// Should be higher than pure geometric (compensates weakness)
		expect(variance).toBeGreaterThan(geometric);
		// But not as high as pure arithmetic (still some penalty)
		expect(variance).toBeLessThan(arithmetic);
	});

	test('two weaknesses with one strength still penalized', () => {
		// Two weaknesses (0.3, 0.3), one strength (0.9)
		const result = varianceAdaptiveAggregate([
			[0.3, 1],
			[0.3, 1],
			[0.9, 1],
		]);

		// Should still be around 0.5 or lower (can't compensate two weaknesses well)
		expect(result).toBeLessThan(0.55);
	});

	test('consistent excellence preserved', () => {
		// All high scores (0.9, 0.9, 0.9)
		// CV = 0 -> geometric mean preserved
		const result = varianceAdaptiveAggregate([
			[0.9, 1],
			[0.9, 1],
			[0.9, 1],
		]);
		expect(result).toBeCloseTo(0.9, 2);
	});

	test('adaptiveness parameter affects compensation', () => {
		const values: Array<[number, number]> = [
			[0.3, 1],
			[0.9, 1],
			[0.9, 1],
		];

		// Conservative (adaptiveness = 1.0) -> less compensation
		const conservative = varianceAdaptiveAggregate(values, 1.0);
		// Aggressive (adaptiveness = 4.0) -> more compensation
		const aggressive = varianceAdaptiveAggregate(values, 4.0);

		// Aggressive should give higher score (more compensation)
		expect(aggressive).toBeGreaterThan(conservative);
	});

	test('respects weights in aggregation', () => {
		// Heavy weight on strength (0.9 at 0.5 weight)
		const heavyStrength = varianceAdaptiveAggregate([
			[0.3, 0.25],
			[0.9, 0.5],
			[0.9, 0.25],
		]);

		// Heavy weight on weakness (0.3 at 0.5 weight)
		const heavyWeakness = varianceAdaptiveAggregate([
			[0.3, 0.5],
			[0.9, 0.25],
			[0.9, 0.25],
		]);

		// Heavy weight on strength should score higher
		expect(heavyStrength).toBeGreaterThan(heavyWeakness);
	});
});

// =============================================================================
// ASSESSED SCORE RECALCULATION
// =============================================================================

describe('recalcAssessedScores', () => {
	// Helper for minimal valid assessment (scoreAdjustment is required)
	const createAssessment = (overrides: Partial<{ recommendation: string; scoreAdjustment: number }> = {}) => ({
		maintenance: 'good' as const,
		lightAndSpace: 'Bright rooms with good natural light',
		photoAnalysis: 'Photos show property accurately',
		recommendation: (overrides.recommendation ?? 'recommend') as 'recommend' | 'strong-recommend' | 'neutral' | 'avoid',
		familySuitability: 'good' as const,
		reasoning: 'Well-maintained property suitable for family',
		scoreAdjustment: overrides.scoreAdjustment ?? 0,
	});

	// Helper for scores object
	const createScores = (overall: number) => ({
		_overall: overall,
		confidence: 0.8,
		affordability: overall,
		location: overall,
		liveability: overall,
		factors: {
			monthlyRent: 1000,
			pricePercentile: 50,
			floorAreaSqm: null,
			floorAreaPercentile: null,
			epcBand: 'C' as const,
			epcNumeric: 70,
			trueMonthlyCost: 1070,
			trueCostPercentile: 50,
			stationMiles: 0.5,
			stationPercentile: 80,
			gigabitPct: 80,
			regionName: 'York',
			priorityScore: 95,
			gardenType: 'private' as const,
			heatingType: 'gas' as const,
			petPolicy: 'unknown' as const,
			propertyType: 'terraced',
			bedrooms: 2,
			imdDecile: null,
			crimeRatePer1k: null,
			crimeRatePercentile: null,
		},
		penalties: { epc: 1.0, garden: 1.0, pets: 1.0, combined: 1.0 },
		context: {
			configHash: 'test-hash',
			percentiles: {
				prices: { min: 0, max: 0, mean: 0, median: 0, stdDev: 0 },
				trueCosts: { min: 0, max: 0, mean: 0, median: 0, stdDev: 0 },
				floorAreas: { min: 0, max: 0, mean: 0, median: 0, stdDev: 0 },
				stationDistances: { min: 0, max: 0, mean: 0, median: 0, stdDev: 0 },
				crimeRates: { min: 0, max: 0, mean: 0, median: 0, stdDev: 0 },
			},
		},
	});

	test('updates assessedScore when algo score changes', () => {
		const scores = createScores(70);
		const listing = createListing({
			scores,
			assessment: createAssessment({ scoreAdjustment: 10 }),
			assessedScore: 80, // stale: was 70 + 10
		});

		// Simulate algo score changing to 60 (e.g., after new listings added)
		scores._overall = 60;

		recalcAssessedScores([listing]);

		// assessedScore should now be 60 + 10 = 70
		expect(listing.assessedScore).toBe(70);
	});

	test('applies zero scoreAdjustment correctly', () => {
		const scores = createScores(70);
		const listing = createListing({
			scores,
			assessment: createAssessment({ scoreAdjustment: 0 }),
			assessedScore: 70,
		});

		// Algo score changes to 65
		scores._overall = 65;

		recalcAssessedScores([listing]);

		// No adjustment, so assessedScore = algoScore
		expect(listing.assessedScore).toBe(65);
	});

	test('skips listings without assessment', () => {
		const listing = createListing({
			scores: createScores(70),
			assessment: null,
			assessedScore: null,
		});

		recalcAssessedScores([listing]);

		expect(listing.assessedScore).toBeNull();
	});

	test('skips listings without scores', () => {
		const listing = createListing({
			scores: null,
			assessment: createAssessment({ scoreAdjustment: 5 }),
			assessedScore: 75,
		});

		recalcAssessedScores([listing]);

		// Should remain unchanged (no scores to recalc from)
		expect(listing.assessedScore).toBe(75);
	});

	test('clamps assessedScore to 0-100 range', () => {
		const scores = createScores(95);
		const listing = createListing({
			scores,
			assessment: createAssessment({ recommendation: 'strong-recommend', scoreAdjustment: 10 }),
			assessedScore: 100,
		});

		recalcAssessedScores([listing]);

		// 95 + 10 = 105, should clamp to 100
		expect(listing.assessedScore).toBe(100);
	});
});
