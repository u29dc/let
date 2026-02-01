import { describe, expect, test } from 'bun:test';
import type { Scores } from '@let/core/pipeline/score';
import { computeStats, filterByMinScore, filterByRegion, filterListings, findListingById, formatStation, formatTableRow, queryListings, sortListings, truncate } from '@let/core/pipeline/view';
import type { Listing } from '@let/core/schema';

// Helper to create a minimal score object for testing
function createScores(overall: number): Scores {
	return {
		_overall: overall,
		confidence: 0.85,
		affordability: 70,
		location: 75,
		liveability: 80,
		factors: {
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
			gardenType: 'private',
			heatingType: 'gas',
			petPolicy: 'unknown',
			propertyType: 'terraced',
			bedrooms: 2,
			imdDecile: null,
			crimeRatePer1k: null,
			crimeRatePercentile: null,
		},
		penalties: {
			epc: 1.0,
			garden: 1.0,
			pets: 1.0,
			combined: 1.0,
		},
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
	};
}

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
		price: 1200,
		priceDisplay: '1,200 pcm',
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

describe('truncate', () => {
	test('returns string unchanged if shorter than max', () => {
		expect(truncate('hello', 10)).toBe('hello');
	});

	test('truncates with ellipsis when longer than max', () => {
		expect(truncate('hello world', 8)).toBe('hello...');
	});

	test('handles exact length', () => {
		expect(truncate('hello', 5)).toBe('hello');
	});
});

describe('filterByRegion', () => {
	const listings = [
		createListing({ id: '1', address: '123 Main St, Sheffield' }),
		createListing({ id: '2', address: '456 High St, Manchester' }),
		createListing({ id: '3', address: '789 Park Rd, Sheffield' }),
	];

	test('filters by exact region match', () => {
		const result = filterByRegion(listings, 'Sheffield');
		expect(result).toHaveLength(2);
		expect(result.map((l) => l.id)).toEqual(['1', '3']);
	});

	test('filters case-insensitively', () => {
		const result = filterByRegion(listings, 'sheffield');
		expect(result).toHaveLength(2);
	});

	test('filters by partial match', () => {
		const result = filterByRegion(listings, 'Manch');
		expect(result).toHaveLength(1);
		expect(result[0]?.id).toBe('2');
	});

	test('returns empty array for no matches', () => {
		const result = filterByRegion(listings, 'London');
		expect(result).toHaveLength(0);
	});
});

describe('filterByMinScore', () => {
	const listings = [
		createListing({ id: '1', scores: createScores(80) }),
		createListing({ id: '2', scores: createScores(50) }),
		createListing({ id: '3', scores: createScores(65) }),
		createListing({ id: '4', scores: null }),
	];

	test('filters by minimum score', () => {
		const result = filterByMinScore(listings, 60);
		expect(result).toHaveLength(2);
		expect(result.map((l) => l.id)).toEqual(['1', '3']);
	});

	test('treats null scores as 0', () => {
		const result = filterByMinScore(listings, 1);
		expect(result).toHaveLength(3); // id '4' has null scores = 0, excluded
	});

	test('returns all when threshold is 0', () => {
		const result = filterByMinScore(listings, 0);
		expect(result).toHaveLength(4); // includes null scores (treated as 0)
	});
});

describe('filterListings', () => {
	const listings = [
		createListing({
			id: '1',
			address: '123 Main St, Sheffield',
			scores: createScores(80),
		}),
		createListing({
			id: '2',
			address: '456 High St, Manchester',
			scores: createScores(50),
		}),
		createListing({
			id: '3',
			address: '789 Park Rd, Sheffield',
			scores: createScores(65),
		}),
	];

	test('applies region filter', () => {
		const result = filterListings(listings, { region: 'Sheffield' });
		expect(result).toHaveLength(2);
	});

	test('applies score filter', () => {
		const result = filterListings(listings, { minScore: 60 });
		expect(result).toHaveLength(2);
	});

	test('combines filters', () => {
		const result = filterListings(listings, { region: 'Sheffield', minScore: 70 });
		expect(result).toHaveLength(1);
		expect(result[0]?.id).toBe('1');
	});

	test('returns all with empty filters', () => {
		const result = filterListings(listings, {});
		expect(result).toHaveLength(3);
	});
});

describe('sortListings', () => {
	const listings = [
		createListing({
			id: '1',
			price: 1000,
			bedrooms: 2,
			scores: createScores(80),
			listedDate: '2024-01-15',
		}),
		createListing({
			id: '2',
			price: 1500,
			bedrooms: 3,
			scores: createScores(50),
			listedDate: '2024-01-20',
		}),
		createListing({
			id: '3',
			price: 800,
			bedrooms: 1,
			scores: createScores(65),
			listedDate: '2024-01-10',
		}),
	];

	test('sorts by score descending (default)', () => {
		const result = sortListings(listings, 'score');
		expect(result.map((l) => l.id)).toEqual(['1', '3', '2']);
	});

	test('sorts by score ascending', () => {
		const result = sortListings(listings, 'score', false);
		expect(result.map((l) => l.id)).toEqual(['2', '3', '1']);
	});

	test('sorts by price descending', () => {
		const result = sortListings(listings, 'price');
		expect(result.map((l) => l.id)).toEqual(['2', '1', '3']);
	});

	test('sorts by price ascending', () => {
		const result = sortListings(listings, 'price', false);
		expect(result.map((l) => l.id)).toEqual(['3', '1', '2']);
	});

	test('sorts by bedrooms', () => {
		const result = sortListings(listings, 'bedrooms');
		expect(result.map((l) => l.id)).toEqual(['2', '1', '3']);
	});

	test('sorts by date', () => {
		const result = sortListings(listings, 'date');
		expect(result.map((l) => l.id)).toEqual(['2', '1', '3']);
	});

	test('does not mutate original array', () => {
		const original = [...listings];
		sortListings(listings, 'price');
		expect(listings.map((l) => l.id)).toEqual(original.map((l) => l.id));
	});
});

describe('queryListings', () => {
	const listings = [
		createListing({
			id: '1',
			address: '123 Main St, Sheffield',
			price: 1000,
			scores: createScores(80),
		}),
		createListing({
			id: '2',
			address: '456 High St, Manchester',
			price: 1500,
			scores: createScores(50),
		}),
		createListing({
			id: '3',
			address: '789 Park Rd, Sheffield',
			price: 800,
			scores: createScores(65),
		}),
		createListing({
			id: '4',
			address: '321 Oak Ave, Sheffield',
			price: 1200,
			scores: createScores(75),
		}),
	];

	test('filters, sorts, and limits', () => {
		const result = queryListings(listings, { region: 'Sheffield', top: 2 }, 'score');
		expect(result).toHaveLength(2);
		expect(result.map((l) => l.id)).toEqual(['1', '4']); // top 2 by score in Sheffield
	});

	test('applies all options together', () => {
		const result = queryListings(listings, { minScore: 60, top: 2 }, 'price', false);
		expect(result).toHaveLength(2);
		expect(result.map((l) => l.id)).toEqual(['3', '1']); // lowest prices with score >= 60
	});
});

describe('computeStats', () => {
	const listings = [
		createListing({
			id: '1',
			address: '123 Main St, Sheffield',
			price: 1000,
			bedrooms: 2,
			scores: createScores(80),
		}),
		createListing({
			id: '2',
			address: '456 High St, Manchester',
			price: 1500,
			bedrooms: 3,
			scores: createScores(50),
		}),
		createListing({
			id: '3',
			address: '789 Park Rd, Sheffield',
			price: 800,
			bedrooms: 2,
			scores: createScores(65),
		}),
		createListing({
			id: '4',
			address: '321 Oak Ave, Sheffield',
			price: 1200,
			bedrooms: 2,
			scores: createScores(75),
		}),
	];

	test('computes total correctly', () => {
		const stats = computeStats(listings);
		expect(stats.total).toBe(4);
	});

	test('computes region breakdown', () => {
		const stats = computeStats(listings);
		expect(stats.byRegion).toHaveLength(2);
		expect(stats.byRegion[0]).toEqual({ region: 'Sheffield', count: 3, percent: 75 });
		expect(stats.byRegion[1]).toEqual({ region: 'Manchester', count: 1, percent: 25 });
	});

	test('computes bedroom breakdown', () => {
		const stats = computeStats(listings);
		expect(stats.byBedrooms).toHaveLength(2);
		expect(stats.byBedrooms.find((b) => b.bedrooms === 2)?.count).toBe(3);
		expect(stats.byBedrooms.find((b) => b.bedrooms === 3)?.count).toBe(1);
	});

	test('computes price statistics', () => {
		const stats = computeStats(listings);
		expect(stats.price.min).toBe(800);
		expect(stats.price.max).toBe(1500);
		expect(stats.price.avg).toBe(1125); // (800+1000+1200+1500)/4
		expect(stats.price.median).toBe(1100); // (1000+1200)/2
	});

	test('computes score statistics', () => {
		const stats = computeStats(listings);
		expect(stats.score.min).toBe(50);
		expect(stats.score.max).toBe(80);
		expect(stats.score.avg).toBe(68); // (50+65+75+80)/4 = 67.5 -> 68
		expect(stats.score.median).toBe(70); // (65+75)/2
	});

	test('computes score distribution', () => {
		const stats = computeStats(listings);
		const excellent = stats.scoreDistribution.find((d) => d.label === 'Excellent');
		const good = stats.scoreDistribution.find((d) => d.label === 'Good');
		const average = stats.scoreDistribution.find((d) => d.label === 'Average');
		expect(excellent?.count).toBe(1); // 80
		expect(good?.count).toBe(2); // 65, 75
		expect(average?.count).toBe(1); // 50
	});

	test('handles empty listings', () => {
		const stats = computeStats([]);
		expect(stats.total).toBe(0);
		expect(stats.byRegion).toHaveLength(0);
		expect(stats.price.min).toBe(0);
	});
});

describe('formatStation', () => {
	test('formats station with distance', () => {
		const listing = createListing({
			nearestStations: [{ name: 'Sheffield Station', distance: 0.5, unit: 'miles' }],
		});
		expect(formatStation(listing)).toBe('Sheffield Station         (0.5mi)');
	});

	test('returns dash when no stations', () => {
		const listing = createListing({ nearestStations: [] });
		expect(formatStation(listing)).toBe('--');
	});

	test('truncates long station names', () => {
		const listing = createListing({
			nearestStations: [{ name: 'Very Long Station Name Here', distance: 1.234, unit: 'miles' }],
		});
		expect(formatStation(listing)).toBe('Very Long Station Name... (1.2mi)');
	});

	test('does not truncate short station names', () => {
		const listing = createListing({
			nearestStations: [{ name: 'York Station', distance: 0.8, unit: 'miles' }],
		});
		expect(formatStation(listing)).toBe('York Station              (0.8mi)');
	});
});

describe('formatTableRow', () => {
	test('formats listing as table row', () => {
		const listing = createListing({
			id: '12345',
			address: 'A Very Long Address That Should Be Truncated, Sheffield',
			price: 950,
			priceDisplay: '950 pcm',
			bedrooms: 3,
			scores: createScores(72),
			nearestStations: [{ name: 'Central Station', distance: 0.3, unit: 'miles' }],
		});
		const row = formatTableRow(listing);
		expect(row.id).toBe('12345');
		expect(row.address.length).toBeLessThanOrEqual(45);
		expect(row.price).toBe(950);
		expect(row.bedrooms).toBe(3);
		expect(row.score).toBe(72);
		expect(row.station).toContain('(0.3mi)');
		expect(row.region).toBe('Sheffield');
	});
});

describe('findListingById', () => {
	const listings = [createListing({ id: '123' }), createListing({ id: '456' }), createListing({ id: '789' })];

	test('finds listing by id', () => {
		const result = findListingById(listings, '456');
		expect(result?.id).toBe('456');
	});

	test('returns undefined when not found', () => {
		const result = findListingById(listings, '999');
		expect(result).toBeUndefined();
	});
});
