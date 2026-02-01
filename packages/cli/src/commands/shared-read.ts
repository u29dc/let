/**
 * Shared read-only utilities for CLI commands
 * Minimal imports - no heavy dependencies (scraper, EPC, Notion, etc.)
 */

import { existsSync } from 'node:fs';
import { isAbsolute, join } from 'node:path';
import { loadListingsFile } from '@let/core/db';
import type { Listing, ListingsFile } from '@let/core/schema';
import { log } from '@let/core/utils/logger';

/**
 * Resolve root directory from LET_HOME env var or fallback to monorepo structure.
 */
function resolveRootDir(): string {
	const letHome = process.env['LET_HOME'];
	if (letHome) {
		return isAbsolute(letHome) ? letHome : join(process.cwd(), letHome);
	}
	// Fallback: 4 levels up from packages/cli/src/commands/
	return join(import.meta.dirname, '..', '..', '..', '..');
}

/** Root directory (from LET_HOME env var or monorepo structure) */
export const ROOT_DIR = resolveRootDir();

/** Cache directory for PAGE_MODEL JSON (gitignored) */
export const CACHE_DIR = join(ROOT_DIR, '.cache');

/** Data directory */
export const DATA_DIR = join(ROOT_DIR, 'data');

/** Path to let.db (SQLite) */
export const LISTINGS_DB_PATH = join(DATA_DIR, 'let.db');
/** Path to let.db.json (export only) */
export const LISTINGS_JSON_PATH = join(DATA_DIR, 'let.db.json');

/** Path to let.config.toml */
export const CONFIG_PATH = join(DATA_DIR, 'let.config.toml');

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
	try {
		const data = loadListingsFile(LISTINGS_DB_PATH) as Partial<ListingsFile>;
		return {
			listings: data.listings ?? [],
			searchUrls: data.searchUrls ?? [],
			locations: data.locations ?? [],
			lastSearchTotal: data.lastSearchTotal ?? 0,
		};
	} catch (error) {
		if (error instanceof Error && error.message.includes('no such column')) {
			log.cli.error('Database schema is outdated; run `let ops migrate` to upgrade IDs and schema', { error: error.message });
			process.exit(1);
		}
		if (options.allowEmptyOnError) {
			if (!existsSync(LISTINGS_DB_PATH)) {
				log.cli.info('No existing database found, starting fresh');
				return { listings: [], searchUrls: [], locations: [], lastSearchTotal: 0 };
			}
			log.cli.error('Database exists but failed to load - refusing to proceed to prevent data loss', {
				error: String(error),
				path: LISTINGS_DB_PATH,
				hint: 'Check let.db.bak for recovery or delete let.db to start fresh',
			});
			process.exit(1);
		}
		log.cli.error('Failed to load listings database', { error: String(error), path: LISTINGS_DB_PATH });
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
