/**
 * Region name extraction helpers
 */

const DEFAULT_REGIONS = [
	'York',
	'Durham',
	'Stamford',
	'Brighton',
	'Harrogate',
	'Newcastle',
	'Liverpool',
	'Morpeth',
	'Lancaster',
	'Folkestone',
	'Leicester',
	'Nottingham',
	'Sheffield',
	'Swansea',
	'Leeds',
	'Manchester',
];

/**
 * Extract region name from region string
 */
export function extractNameFromRegion(region: string | null): string | null {
	if (!region) return null;
	const parts = region.split(',');
	if (parts.length > 0 && parts[0]) {
		return parts[0].trim();
	}
	return region.trim();
}

/**
 * Match a region name against a configured list (case-insensitive)
 */
export function matchRegionName(region: string | null, regions: string[]): string | null {
	if (!region) return null;

	const normalized = region.trim().toLowerCase();
	for (const candidate of regions) {
		if (candidate.toLowerCase() === normalized) {
			return candidate;
		}
	}

	return null;
}

/**
 * Check if a part starts with a region name at a word boundary.
 * This allows "Newcastle upon Tyne" but rejects "Yorkshire" for "York".
 */
function matchesRegionAtStart(part: string, region: string): boolean {
	const regionLower = region.toLowerCase();
	if (!part.startsWith(regionLower)) return false;
	const nextChar = part[regionLower.length];
	return nextChar === undefined || !/[a-z]/.test(nextChar);
}

/**
 * Extract region name from address string
 *
 * Splits address by comma and checks if any part starts with a region name
 * at a word boundary. This prevents false matches like "York" in "South Yorkshire"
 * while allowing "Newcastle" to match "Newcastle upon Tyne".
 */
export function extractNameFromAddress(address: string | null, regions: string[] = DEFAULT_REGIONS): string | null {
	if (!address) return null;

	const parts = address.split(',').map((p) => p.trim().toLowerCase());

	for (const part of parts) {
		for (const region of regions) {
			if (matchesRegionAtStart(part, region)) {
				return region;
			}
		}
	}
	return null;
}
