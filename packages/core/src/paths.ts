/**
 * Cross-platform path resolution (shared between CLI and core)
 *
 * Single source of truth for all file paths. CLI primes at startup
 * with overrides; core enrichment code calls paths() to get cached result.
 *
 * Precedence (highest to lowest):
 *  1. CLI flags (PathOverrides)
 *  2. Category env vars (LET_DATA_DIR, LET_CONFIG_DIR, etc.)
 *  3. LET_HOME or TOOLS_HOME env var, defaulting to ~/.tools/let
 */

import { homedir } from 'node:os';
import { isAbsolute, join } from 'node:path';

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

export interface PathOverrides {
	dataDir?: string | undefined;
	configDir?: string | undefined;
	cacheDir?: string | undefined;
	sourcesDir?: string | undefined;
}

export interface ResolvedPaths {
	/** Directory containing config file */
	config: string;
	/** Directory containing let.db */
	data: string;
	/** Directory containing {portalId}/ cache entries */
	cache: string;
	/** Directory containing *.db source databases */
	sources: string;
}

export interface DerivedPaths {
	/** Config file path (let.config.toml) */
	configFile: string;
	/** Template config file path (dev only) */
	templateFile: string;
	/** .env file path */
	envFile: string;
	/** Listings database path */
	database: string;
	/** Listings database backup path */
	backup: string;
	/** JSON export path */
	jsonExport: string;
	/** Source database path for a given name */
	sourceDb(name: string): string;
	/** Cache directory for a portal ID */
	cacheDir(id: string): string;
	/** Cache entry (data.json) for a portal ID */
	cacheEntry(id: string): string;
}

// ---------------------------------------------------------------------------
// Resolution
// ---------------------------------------------------------------------------

function resolveAbsoluteOrCwd(value: string): string {
	return isAbsolute(value) ? value : join(process.cwd(), value);
}

/**
 * Build derived paths from resolved directories.
 */
function buildDerived(resolved: ResolvedPaths): DerivedPaths {
	const configFileName = 'let.config.toml';
	return {
		configFile: join(resolved.config, configFileName),
		templateFile: join(resolved.config, 'let.config.template.toml'),
		envFile: join(resolved.config, '.env'),
		database: join(resolved.data, 'let.db'),
		backup: join(resolved.data, 'let.db.bak'),
		jsonExport: join(resolved.data, 'let.db.json'),
		sourceDb(name: string): string {
			return join(resolved.sources, `${name}.db`);
		},
		cacheDir(id: string): string {
			return join(resolved.cache, id);
		},
		cacheEntry(id: string): string {
			return join(resolved.cache, id, 'data.json');
		},
	};
}

// ---------------------------------------------------------------------------
// Cached singleton
// ---------------------------------------------------------------------------

let cached: { resolved: ResolvedPaths; derived: DerivedPaths } | null = null;

type DirSet = { config: string; data: string; cache: string; sources: string };

/**
 * Compute base defaults from LET_HOME / TOOLS_HOME, defaulting to ~/.tools/let.
 */
function resolveDefaults(): { defaults: DirSet } {
	const letHome = process.env['LET_HOME'] || join(process.env['TOOLS_HOME'] || join(homedir(), '.tools'), 'let');
	return {
		defaults: {
			config: join(letHome, 'data'),
			data: join(letHome, 'data'),
			cache: join(letHome, 'cache'),
			sources: join(letHome, 'sources'),
		},
	};
}

/**
 * Apply category env var if set, otherwise return the default value.
 */
function envOrDefault(envKey: string, fallback: string): string {
	const value = process.env[envKey];
	return value ? resolveAbsoluteOrCwd(value) : fallback;
}

/**
 * Resolve all paths using the precedence chain.
 * Result is cached after first call. Subsequent calls return the cache
 * unless overrides are provided (which forces re-resolution).
 */
export function resolvePaths(overrides?: PathOverrides): { resolved: ResolvedPaths; derived: DerivedPaths } {
	if (cached && !overrides) return cached;

	const { defaults } = resolveDefaults();

	// Apply category env vars over defaults
	const resolved: ResolvedPaths = {
		config: envOrDefault('LET_CONFIG_DIR', defaults.config),
		data: envOrDefault('LET_DATA_DIR', defaults.data),
		cache: envOrDefault('LET_CACHE_DIR', defaults.cache),
		sources: envOrDefault('LET_SOURCES_DIR', defaults.sources),
	};

	// Apply CLI flag overrides (highest priority)
	if (overrides?.configDir) resolved.config = resolveAbsoluteOrCwd(overrides.configDir);
	if (overrides?.dataDir) resolved.data = resolveAbsoluteOrCwd(overrides.dataDir);
	if (overrides?.cacheDir) resolved.cache = resolveAbsoluteOrCwd(overrides.cacheDir);
	if (overrides?.sourcesDir) resolved.sources = resolveAbsoluteOrCwd(overrides.sourcesDir);

	const result = { resolved, derived: buildDerived(resolved) };
	cached = result;
	return result;
}

/**
 * Get cached resolved paths. Throws if resolvePaths() has not been called yet.
 */
export function paths(): { resolved: ResolvedPaths; derived: DerivedPaths } {
	if (!cached) {
		// Auto-resolve with no overrides (e.g. core enrichment calling before CLI priming)
		return resolvePaths();
	}
	return cached;
}

/**
 * Reset the cached singleton (test-only).
 */
export function resetPaths(): void {
	cached = null;
}
