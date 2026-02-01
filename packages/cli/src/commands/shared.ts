/**
 * Shared utilities and constants for CLI commands
 *
 * Re-exports from split modules:
 * - shared-read.ts: Minimal imports for read-only operations (view, help)
 * - shared-write.ts: Heavy imports for write operations (fetch, assess, output, ops)
 *
 * Import directly from shared-read.ts for faster startup on read-only commands.
 */

export { CACHE_DIR, CONFIG_PATH, DATA_DIR, LISTINGS_DB_PATH, LISTINGS_JSON_PATH, loadExistingListings, ROOT_DIR, setupSignalHandlers } from './shared-read.js';

export type { ProcessListingOptions } from './shared-write.js';
export { cachePageModel, downloadListingAssets, getCachedHtml, loadConfigOrExit, processListing, saveListingsFile } from './shared-write.js';
