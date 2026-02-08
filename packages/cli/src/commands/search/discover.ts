/**
 * `search discover` — Discover listing IDs from configured search locations.
 *
 * Uses config locations and filters. Returns portal IDs only, no persistence.
 * Supports optional --region filter to search a subset of locations.
 */

import type { SearchConfig, SearchFilters } from '@let/core/config';
import { loadConfig, resetConfigCache } from '@let/core/config';
import { paths } from '@let/core/paths';
import { type ApiSearchParams, searchListingsApi, setApiDelay, setApiMaxRetries, setFetchDelay, setFetchMaxRetries } from '@let/core/pipeline/fetch';
import { log } from '@let/core/utils/logger';
import { fail, isJsonMode, ok, rethrowCapture } from '../../envelope.js';
import { defineToolCommand } from '../../tool.js';

function dedupeIds(ids: string[]): string[] {
	const seen = new Set<string>();
	const unique: string[] = [];
	for (const id of ids) {
		if (!id || seen.has(id)) continue;
		seen.add(id);
		unique.push(id);
	}
	return unique;
}

function parseCommaSeparated(value: string): string[] {
	return value === 'none' ? [] : value.split(',').map((t) => t.trim());
}

function applyFilterOverrides(filters: SearchFilters, args: Record<string, unknown>): SearchFilters {
	let result = filters;
	const propertyTypes = args['property-types'];
	const mustHave = args['must-have'];
	const dontShow = args['dont-show'];
	if (typeof propertyTypes === 'string') {
		result = { ...result, propertyTypes: propertyTypes.split(',').map((t) => t.trim()) };
	}
	if (typeof mustHave === 'string') {
		result = { ...result, mustHave: parseCommaSeparated(mustHave) };
	}
	if (typeof dontShow === 'string') {
		result = { ...result, dontShow: parseCommaSeparated(dontShow) };
	}
	return result;
}

function buildSearchParams(locationId: string, filters: SearchFilters, maxPerLocation: number): ApiSearchParams {
	return {
		locationIdentifier: locationId,
		minBedrooms: filters.minBedrooms,
		maxBedrooms: filters.maxBedrooms,
		minPrice: filters.minPrice,
		maxPrice: filters.maxPrice,
		propertyTypes: filters.propertyTypes,
		includeLetAgreed: filters.includeLetAgreed,
		radius: filters.radius,
		dontShow: filters.dontShow,
		mustHave: filters.mustHave,
		numberOfPropertiesPerPage: Math.min(maxPerLocation, 24),
	};
}

async function searchLocations(filters: SearchFilters, locations: SearchConfig['locations'], maxPerLocation: number) {
	const allIds: string[] = [];
	const locationResults: Array<{ name: string; id: string; count: number }> = [];

	for (const loc of locations) {
		log.cli.info(`Searching ${loc.name}...`);
		const params = buildSearchParams(loc.id, filters, maxPerLocation);
		const result = await searchListingsApi(params);

		if (result.success) {
			allIds.push(...result.listingIds);
			locationResults.push({ name: loc.name, id: loc.id, count: result.listingIds.length });
			log.cli.info(`  Found ${result.listingIds.length} of ${result.totalResults} total`);
		} else {
			log.cli.warn(`  Search failed for ${loc.name}: ${result.error}`);
			locationResults.push({ name: loc.name, id: loc.id, count: 0 });
		}
	}

	return { ids: dedupeIds(allIds), locationResults };
}

export const searchDiscoverCommand = defineToolCommand(
	{
		name: 'search.discover',
		command: 'let search discover',
		category: 'search',
		outputFields: ['ids', 'total', 'locations'],
		idempotent: true,
		rateLimit: 'config fetch.delayMs per request',
		example: 'let search discover --location REGION^904 --property-types flat --must-have none --json',
	},
	{
		meta: {
			name: 'discover',
			description: 'Discover listing IDs from search locations',
		},
		args: {
			region: {
				type: 'string' as const,
				description: 'Filter to specific location name',
			},
			location: {
				type: 'string' as const,
				description: 'Raw locationIdentifier for ad-hoc search (e.g., REGION^904)',
			},
			'property-types': {
				type: 'string' as const,
				description: 'Override property types (comma-separated, e.g., flat,apartment)',
			},
			'must-have': {
				type: 'string' as const,
				description: 'Override mustHave filters (comma-separated, or "none" to clear)',
			},
			'dont-show': {
				type: 'string' as const,
				description: 'Override dontShow filters (comma-separated, or "none" to clear)',
			},
			'location-name': {
				type: 'string' as const,
				description: 'Display name for ad-hoc location (used in output)',
			},
			limit: {
				type: 'string' as const,
				description: 'Max listings per location',
			},
			json: {
				type: 'boolean' as const,
				description: 'Output as JSON envelope',
				default: false,
			},
		},
		async run({ args }) {
			const start = performance.now();
			const jsonMode = isJsonMode();
			const p = paths();

			try {
				resetConfigCache();
				const config = await loadConfig(p.derived.configFile);

				setApiDelay(config.fetch.delayMs);
				setApiMaxRetries(config.fetch.maxRetries);
				setFetchDelay(config.fetch.delayMs);
				setFetchMaxRetries(config.fetch.maxRetries);

				let locations = config.search.locations;
				if (args.location) {
					locations = [{ id: args.location, name: args['location-name'] || args.location }];
				} else if (args.region) {
					const regionLower = args.region.toLowerCase();
					locations = locations.filter((loc) => loc.name.toLowerCase().includes(regionLower));
					if (locations.length === 0) {
						if (jsonMode) {
							fail('search.discover', 'NO_MATCH', `No locations match "${args.region}"`, 'Check config search.locations', start);
						}
						log.cli.error(`No locations match "${args.region}"`);
						process.exit(1);
					}
				}

				const filters = applyFilterOverrides(config.search.filters, args);

				const maxPerLocation = args.limit ? Number.parseInt(args.limit, 10) : config.fetch.maxListings;
				const { ids, locationResults } = await searchLocations(filters, locations, maxPerLocation);

				if (jsonMode) {
					ok('search.discover', { ids, total: ids.length, locations: locationResults }, start);
				}

				log.cli.info(`Discovered ${ids.length} unique listing IDs across ${locations.length} location(s)`);
			} catch (error) {
				rethrowCapture(error);
				const message = error instanceof Error ? error.message : String(error);
				if (jsonMode) {
					fail('search.discover', 'SEARCH_ERROR', `Search failed: ${message}`, 'Check config and network', start);
				}
				log.cli.error(`Search failed: ${message}`);
				process.exit(1);
			}
		},
	},
);
