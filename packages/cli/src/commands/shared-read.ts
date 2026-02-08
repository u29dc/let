/**
 * Shared read-only utilities for CLI commands
 * Minimal imports - no heavy dependencies (scraper, EPC, Notion, etc.)
 */

import { existsSync } from 'node:fs';
import { loadListingsFile } from '@let/core/db';
import { paths } from '@let/core/paths';
import type { Listing, ListingsFile } from '@let/core/schema';
import { log } from '@let/core/utils/logger';

/**
 * Load existing listings from data/let.db
 * Returns empty defaults if database is empty or invalid
 */
export function loadExistingListings(options: { allowEmptyOnError?: boolean } = {}): {
	listings: Listing[];
	searchUrls: string[];
	locations: string[];
	lastSearchTotal: number;
} {
	const dbPath = paths().derived.database;
	try {
		const data = loadListingsFile(dbPath) as Partial<ListingsFile>;
		return {
			listings: data.listings ?? [],
			searchUrls: data.searchUrls ?? [],
			locations: data.locations ?? [],
			lastSearchTotal: data.lastSearchTotal ?? 0,
		};
	} catch (error) {
		if (error instanceof Error && error.message.includes('no such column')) {
			log.cli.error('Database schema mismatch - schema may be incompatible with current version', { error: error.message });
			process.exit(1);
		}
		if (options.allowEmptyOnError) {
			if (!existsSync(dbPath)) {
				log.cli.info('No existing database found, starting fresh');
				return { listings: [], searchUrls: [], locations: [], lastSearchTotal: 0 };
			}
			log.cli.error('Database exists but failed to load - refusing to proceed to prevent data loss', {
				error: String(error),
				path: dbPath,
				hint: 'Check let.db.bak for recovery or delete let.db to start fresh',
			});
			process.exit(1);
		}
		log.cli.error('Failed to load listings database', { error: String(error), path: dbPath });
		process.exit(1);
	}
}

/**
 * Graceful shutdown handler for Ctrl+C
 */
export function setupSignalHandlers(): void {
	process.on('SIGINT', () => {
		log.cli.warn('Interrupted by user (Ctrl+C)');
		process.exit(130); // Standard exit code for SIGINT
	});
}
