import { describe, expect, test } from 'bun:test';
import { buildNotionProperties } from '@let/core/pipeline/output';
import type { Scores } from '@let/core/pipeline/score';
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
			petPolicy: 'yes',
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
			deprivation: 1.0,
			highCrime: 1.0,
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
		id: '123456',
		portalIds: { rightmove: overrides.portalIds?.rightmove ?? overrides.id ?? '123456' },
		uprn: null,
		uprnSource: null,
		uprnConfidence: null,
		url: 'https://www.rightmove.co.uk/properties/123456',
		location: { lat: 53.9614, lng: -1.0739, pinType: 'ACCURATE_POINT' },
		postcode: 'YO24 4AB',
		address: '42 Test Street, York',
		googleMapsUrl: 'https://www.google.com/maps/search/?api=1&query=42%20Test%20Street%2C%20York%20YO24%204AB',
		googleMapsStreetViewUrl: 'https://www.google.com/maps/@?api=1&map_action=pano&viewpoint=53.9614,-1.0739',
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
		bedrooms: 3,
		bathrooms: 2,
		propertyType: 'Terraced',
		description: 'A lovely property',
		notes: ['Good condition', 'Private garden'],
		images: [
			{ remote: 'https://media.rightmove.co.uk/1.jpg', local: null },
			{ remote: 'https://media.rightmove.co.uk/2.jpg', local: null },
		],
		floorplan: { remote: 'https://media.rightmove.co.uk/floorplan.jpg', local: null },
		epc: { remote: 'https://media.rightmove.co.uk/epc.jpg', local: null },
		mapViews: { satellite: { remote: null, local: null }, street: { remote: null, local: null } },
		epcRating: 'C',
		floorAreaSqm: 85,
		epcLodgementDate: '2023-01-15',
		epcAddressMatch: true,
		epcSearchUrl: 'https://find-energy-certificate.service.gov.uk/find-a-certificate/search-by-postcode?postcode=YO24%204AB',
		nearestStations: [{ name: 'York', distance: 0.5, unit: 'miles' }],
		gigabitAvailability: 95,
		listedDate: '2024-01-10',
		lettings: { availableDate: '2024-02-01', deposit: 1200 },
		agent: { name: 'Test Agent', phone: '01onal234567' },
		assessment: null,
		assessedAt: null,
		assessedScore: null,
		region: 'York, North Yorkshire',
		scores: createScores(78),
		fetchedAt: new Date().toISOString(),
		extractionStatus: 'success',
		status: 'active',
		...overrides,
	};
}

describe('buildNotionProperties', () => {
	test('builds complete properties from listing', () => {
		const listing = createListing();
		const props = buildNotionProperties(listing);

		// Title
		expect(props['Name']).toEqual({ title: [{ text: { content: '42 Test Street, York' } }] });

		// Numbers
		expect(props['Price']).toEqual({ number: 1200 });
		expect(props['Bedrooms']).toEqual({ number: 3 });
		expect(props['Bathrooms']).toEqual({ number: 2 });
		expect(props['Floor Area']).toEqual({ number: 85 });
		expect(props['Score']).toEqual({ number: 78 });

		// Selects
		expect(props['EPC']).toEqual({ select: { name: 'C' } });
		expect(props['Garden']).toEqual({ select: { name: 'private' } });
		expect(props['Heating']).toEqual({ select: { name: 'gas' } });
		expect(props['Pets']).toEqual({ select: { name: 'yes' } });

		// Rich text
		expect(props['Type']).toEqual({ rich_text: [{ text: { content: 'Terraced' } }] });
		expect(props['Region']).toEqual({ rich_text: [{ text: { content: 'York, North Yorkshire' } }] });
		expect(props['Notes']).toEqual({ rich_text: [{ text: { content: 'Good condition\nPrivate garden' } }] });

		// URL
		expect(props['URL']).toEqual({ url: 'https://www.rightmove.co.uk/properties/123456' });
	});

	test('formats Address Text with coordinates', () => {
		const listing = createListing();
		const props = buildNotionProperties(listing);

		expect(props['Address Text']).toEqual({
			rich_text: [{ text: { content: '42 Test Street, York, YO24 4AB [53.9614,-1.0739]' } }],
		});
	});

	test('builds external file objects for images', () => {
		const listing = createListing();
		const props = buildNotionProperties(listing);

		expect(props['Images']).toEqual({
			files: [
				{ type: 'external', name: 'Image 1', external: { url: 'https://media.rightmove.co.uk/1.jpg' } },
				{ type: 'external', name: 'Image 2', external: { url: 'https://media.rightmove.co.uk/2.jpg' } },
			],
		});
	});

	test('handles missing optional fields', () => {
		const listing = createListing({
			scores: null,
			epcRating: null,
			floorAreaSqm: null,
			notes: [],
			region: null,
			images: [],
		});
		const props = buildNotionProperties(listing);

		expect(props['Score']).toEqual({ number: null });
		expect(props['Floor Area']).toEqual({ number: null });
		expect(props['EPC']).toEqual({ select: null });
		expect(props['Garden']).toEqual({ select: null });
		expect(props['Heating']).toEqual({ select: null });
		expect(props['Pets']).toEqual({ select: null });
		expect(props['Notes']).toEqual({ rich_text: [] });
		expect(props['Region']).toEqual({ rich_text: [] });
		expect(props['Images']).toEqual({ files: [] });
	});

	test('truncates text fields at 2000 chars', () => {
		const longText = 'a'.repeat(2500);
		const listing = createListing({ address: longText });
		const props = buildNotionProperties(listing);

		const titleContent = (props['Name'] as { title: Array<{ text: { content: string } }> }).title[0]?.text.content;
		expect(titleContent?.length).toBe(2000);
	});

	test('handles empty images array', () => {
		const listing = createListing({ images: [] });
		const props = buildNotionProperties(listing);

		expect(props['Images']).toEqual({ files: [] });
	});
});
