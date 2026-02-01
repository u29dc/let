import { describe, expect, test } from 'bun:test';
import { extractBroadband, parseListedDate, parsePrice, scrapeSearchResults, transformPageModel } from '@let/core/pipeline/parse';

describe('parsePrice', () => {
	test('parses monthly rent', () => {
		expect(parsePrice('£1,000 pcm')).toBe(1000);
		expect(parsePrice('£950 pcm')).toBe(950);
		expect(parsePrice('£1,450 pcm')).toBe(1450);
	});

	test('converts weekly to monthly', () => {
		// £200/week * 52/12 = £866.67/month
		expect(parsePrice('£200 pw')).toBe(867);
	});

	test('prefers monthly when both weekly and monthly appear', () => {
		expect(parsePrice('£900 pcm (£210 pw)')).toBe(900);
		expect(parsePrice('£210 pw (£900 pcm)')).toBe(900);
	});

	test('parses per-month variations', () => {
		expect(parsePrice('£1,200 per month')).toBe(1200);
		expect(parsePrice('£1,200 p/m')).toBe(1200);
	});

	test('handles variations', () => {
		expect(parsePrice('£1000')).toBe(1000);
		expect(parsePrice('1,200 pcm')).toBe(1200);
	});

	test('returns undefined for invalid', () => {
		expect(parsePrice('')).toBeUndefined();
		expect(parsePrice('POA')).toBeUndefined();
	});
});

describe('parseListedDate', () => {
	test('parses "Added on" format', () => {
		expect(parseListedDate('Added on 15/01/2024')).toBe('2024-01-15');
		expect(parseListedDate('Added on 01/12/2023')).toBe('2023-12-01');
	});

	test('parses "Reduced on" format', () => {
		expect(parseListedDate('Reduced on 20/06/2024')).toBe('2024-06-20');
	});

	test('returns undefined for invalid', () => {
		expect(parseListedDate('')).toBeUndefined();
		expect(parseListedDate('No date here')).toBeUndefined();
	});
});

describe('extractBroadband', () => {
	test('extracts max speed from array', () => {
		const data = [{ maxDownloadSpeed: 30 }, { maxDownloadSpeed: 100 }, { maxDownloadSpeed: 67 }];
		expect(extractBroadband(data)).toBe(100);
	});

	test('returns undefined for empty array', () => {
		expect(extractBroadband([])).toBeUndefined();
	});

	test('returns undefined for non-array', () => {
		expect(extractBroadband(null)).toBeUndefined();
		expect(extractBroadband({ maxDownloadSpeed: 100 })).toBeUndefined();
	});
});

describe('transformPageModel', () => {
	const validPageModel = {
		propertyData: {
			id: 170448131,
			location: {
				latitude: 53.9614,
				longitude: -1.0739,
				pinType: 'ACCURATE_POINT',
			},
			address: {
				displayAddress: 'High Street, York',
				outcode: 'YO1',
				incode: '7EP',
			},
			prices: {
				primaryPrice: '£1,200 pcm',
			},
			bedrooms: 2,
			bathrooms: 1,
			propertySubType: 'Terraced',
			text: {
				description: '<p>Lovely 2-bed house with garden.</p>',
			},
			keyFeatures: ['Garden', 'Gas heating', 'Double glazing'],
			images: [{ url: 'https://example.com/img1.jpg', caption: 'Living room' }, { url: 'https://example.com/img2.jpg' }],
			floorplans: [{ url: 'https://example.com/floorplan.jpg' }],
			epcGraphs: [{ url: 'https://example.com/epc.png' }],
			nearestStations: [
				{ name: 'York', distance: 0.5, unit: 'miles' },
				{ name: 'Poppleton', distance: 2.1, unit: 'miles' },
			],
			broadband: [{ maxDownloadSpeed: 100 }, { maxDownloadSpeed: 67 }],
			listingHistory: {
				listingUpdateReason: 'Added on 15/01/2024',
			},
			lettings: {
				letAvailableDate: 'Now',
				deposit: 1200,
			},
			customer: {
				branchDisplayName: 'Test Estate Agents',
			},
			contactInfo: {
				telephoneNumbers: {
					localNumber: '01onal 123456',
				},
			},
		},
	};

	test('transforms valid PAGE_MODEL to Listing', () => {
		const result = transformPageModel(validPageModel, '2024-01-20T10:00:00.000Z');

		expect(result.success).toBe(true);
		if (result.success) {
			const listing = result.listing;

			expect(listing.id).toMatch(/^[0-9a-f-]{36}$/i);
			expect(listing.portalIds.rightmove).toBe('170448131');
			expect(listing.url).toBe('https://www.rightmove.co.uk/properties/170448131');
			expect(listing.location.lat).toBe(53.9614);
			expect(listing.location.lng).toBe(-1.0739);
			expect(listing.location.pinType).toBe('ACCURATE_POINT');
			expect(listing.postcode).toBe('YO1 7EP');
			expect(listing.address).toBe('High Street, York');
			expect(listing.googleMapsUrl).toBe('https://www.google.com/maps/place/High%20Street%2C%20York%2C%20YO1%207EP/@53.9614,-1.0739,17z/data=!3m1!1e3');
			expect(listing.googleMapsStreetViewUrl).toBe('https://www.google.com/maps/@?api=1&map_action=pano&viewpoint=53.9614,-1.0739');
			expect(listing.price).toBe(1200);
			expect(listing.priceDisplay).toBe('£1,200 pcm');
			expect(listing.bedrooms).toBe(2);
			expect(listing.bathrooms).toBe(1);
			expect(listing.propertyType).toBe('Terraced');
			expect(listing.description).toContain('lovely 2-bed house');
			expect(listing.notes).toEqual([]);
			expect(listing.images).toHaveLength(2);
			expect(listing.images[0]).toEqual({ remote: 'https://example.com/img1.jpg', local: null });
			expect(listing.floorplan).toEqual({ remote: 'https://example.com/floorplan.jpg', local: null });
			expect(listing.epc).toEqual({ remote: 'https://example.com/epc.png', local: null });
			expect(listing.epcSearchUrl).toBe('https://find-energy-certificate.service.gov.uk/find-a-certificate/search-by-postcode?postcode=YO1%207EP');
			expect(listing.nearestStations).toHaveLength(2);
			expect(listing.gigabitAvailability).toBeNull(); // Populated later by Ofcom lookup
			expect(listing.listedDate).toBe('2024-01-15');
			expect(listing.lettings?.availableDate).toBe('Now');
			expect(listing.lettings?.deposit).toBe(1200);
			expect(listing.agent?.name).toBe('Test Estate Agents');
			expect(listing.extractionStatus).toBe('success');
		}
	});

	test('normalizes station distances to miles', () => {
		const kmPageModel = {
			...validPageModel,
			propertyData: {
				...validPageModel.propertyData,
				nearestStations: [{ name: 'York', distance: 1, unit: 'km' }],
			},
		};

		const result = transformPageModel(kmPageModel, '2024-01-20T10:00:00.000Z');

		expect(result.success).toBe(true);
		if (result.success) {
			const [station] = result.listing.nearestStations;
			expect(station?.distance).toBeCloseTo(0.621371, 6);
			expect(station?.unit).toBe('miles');
		}
	});

	test('returns error for missing propertyData', () => {
		const result = transformPageModel({}, '2024-01-20T10:00:00.000Z');

		expect(result.success).toBe(false);
		if (!result.success) {
			expect(result.error).toContain('propertyData not found');
		}
	});

	test('returns error for missing required fields', () => {
		const result = transformPageModel(
			{
				propertyData: {
					id: 123,
					// Missing location
				},
			},
			'2024-01-20T10:00:00.000Z',
		);

		expect(result.success).toBe(false);
		if (!result.success) {
			expect(result.error).toContain('coordinates not found');
		}
	});
});

describe('scrapeSearchResults', () => {
	test('extracts listing IDs from search page', () => {
		const html = `
      <script id="__NEXT_DATA__" type="application/json">
        {
          "props": {
            "pageProps": {
              "searchResults": {
                "resultCount": "150",
                "properties": [
                  {"id": 170448131},
                  {"id": 170692334},
                  {"id": 170555555}
                ]
              }
            }
          }
        }
      </script>
    `;

		const result = scrapeSearchResults(html);

		expect(result.success).toBe(true);
		if (result.success) {
			expect(result.listingIds).toEqual(['170448131', '170692334', '170555555']);
			expect(result.totalResults).toBe(150);
		}
	});

	test('returns error for missing properties', () => {
		const html = `
      <script id="__NEXT_DATA__" type="application/json">
        {"props": {"pageProps": {}}}
      </script>
    `;

		const result = scrapeSearchResults(html);

		expect(result.success).toBe(false);
		if (!result.success) {
			expect(result.error).toContain('Properties array not found');
		}
	});
});
