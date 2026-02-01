import { afterEach, beforeEach, describe, expect, test } from 'bun:test';
import { enrichWithEpc, fetchEpcByPostcode, resetEpcRateLimiter } from '@let/core/pipeline/enrich';
import type { Listing } from '@let/core/schema';

function setMockFetch(handler: (input: RequestInfo | URL, init?: RequestInit) => Promise<Response>): void {
	const mock = ((input: RequestInfo | URL, init?: RequestInit) => handler(input, init)) as typeof fetch;
	mock.preconnect = () => {};
	globalThis.fetch = mock;
}

const originalFetch = globalThis.fetch;
const originalEmail = process.env['EPC_API_EMAIL'];
const originalKey = process.env['EPC_API_KEY'];

function restoreEnv(): void {
	if (originalEmail === undefined) {
		delete process.env['EPC_API_EMAIL'];
	} else {
		process.env['EPC_API_EMAIL'] = originalEmail;
	}
	if (originalKey === undefined) {
		delete process.env['EPC_API_KEY'];
	} else {
		process.env['EPC_API_KEY'] = originalKey;
	}
}

function createListing(overrides: Partial<Listing> = {}): Listing {
	return {
		id: '123',
		portalIds: { rightmove: overrides.portalIds?.rightmove ?? overrides.id ?? '123' },
		uprn: null,
		uprnSource: null,
		uprnConfidence: null,
		url: 'https://www.rightmove.co.uk/properties/123',
		location: { lat: 53.96, lng: -1.07, pinType: null },
		postcode: 'AB1 2CD',
		address: 'High Street',
		googleMapsUrl: 'https://www.google.com/maps/search/?api=1&query=High%20Street%20AB1%202CD',
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
		epcSearchUrl: 'https://find-energy-certificate.service.gov.uk/find-a-certificate/search-by-postcode?postcode=AB1%202CD',
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

const singleRecordCsv = ['address,postcode,current-energy-rating,total-floor-area,property-type,lodgement-date', '1 High Street,AB1 2CD,B,50,House,2020-01-01'].join('\n');

const medianRecordCsv = [
	'address,postcode,current-energy-rating,total-floor-area,property-type,lodgement-date',
	'1 High Street,AB1 2CD,C,40,House,2020-01-01',
	'3 High Street,AB1 2CD,B,60,House,2020-01-01',
	'5 High Street,AB1 2CD,D,80,House,2020-01-01',
].join('\n');

describe('EPC API', () => {
	beforeEach(() => {
		process.env['EPC_API_EMAIL'] = 'test@example.com';
		process.env['EPC_API_KEY'] = 'test-key';
		resetEpcRateLimiter();
	});

	afterEach(() => {
		globalThis.fetch = originalFetch;
		resetEpcRateLimiter();
		restoreEnv();
	});

	test.serial('fetchEpcByPostcode retries on 429 and succeeds', async () => {
		let calls = 0;
		setMockFetch(async () => {
			calls += 1;
			if (calls === 1) {
				return new Response('rate limit', { status: 429, headers: { 'Retry-After': '0' } });
			}
			return new Response(singleRecordCsv, { status: 200, headers: { 'Content-Type': 'text/csv' } });
		});

		const result = await fetchEpcByPostcode('AB1 2CD');

		expect(calls).toBe(2);
		expect(result.success).toBe(true);
		if (result.success) {
			expect(result.records.length).toBe(1);
			expect(result.records[0]?.epcRating).toBe('B');
		}
	});

	test.serial('enrichWithEpc uses street-median fallback when no house number', async () => {
		setMockFetch(async () => new Response(medianRecordCsv, { status: 200, headers: { 'Content-Type': 'text/csv' } }));

		const listing = createListing({ address: 'High Street' });
		const result = await enrichWithEpc(listing);

		expect(result.success).toBe(true);
		if (result.success) {
			expect(result.matchSource).toBe('street-median');
			expect(result.epc?.floorAreaSqm).toBe(60);
		}
	});
});
