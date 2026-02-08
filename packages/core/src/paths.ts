/**
 * Cross-platform path resolution (shared between CLI and core)
 *
 * Single source of truth for all file paths. CLI primes at startup
 * with overrides; core enrichment code calls paths() to get cached result.
 *
 * Precedence (highest to lowest):
 *  1. CLI flags (PathOverrides)
 *  2. Category env vars (LET_DATA_DIR, LET_CONFIG_DIR, etc.)
 *  3. Dev mode detection (monorepo root via package.json marker)
 *  4. Binary location detection (compiled binary in {skill}/.let/bin/)
 *  5. OS defaults (XDG on Linux, ~/Library on macOS)
 */

import { existsSync, readFileSync } from 'node:fs';
import { homedir, platform } from 'node:os';
import { basename, dirname, isAbsolute, join } from 'node:path';

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
	/** True when running from monorepo checkout */
	isDev: boolean;
}

export interface DerivedPaths {
	/** Config file path (let.config.toml in dev, config.toml installed) */
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
// Dev mode detection
// ---------------------------------------------------------------------------

/**
 * Walk up from startDir (max levels) looking for a package.json with
 * name "let" and a workspaces field. Returns the monorepo root or null.
 */
function detectMonorepoRoot(startDir: string, maxLevels = 5): string | null {
	let dir = startDir;
	for (let i = 0; i < maxLevels; i++) {
		const pkgPath = join(dir, 'package.json');
		if (existsSync(pkgPath)) {
			try {
				const raw = readFileSync(pkgPath, 'utf-8');
				const pkg = JSON.parse(raw) as Record<string, unknown>;
				if (pkg['name'] === 'let' && pkg['workspaces']) {
					return dir;
				}
			} catch {
				// Ignore parse errors
			}
		}
		const parent = join(dir, '..');
		if (parent === dir) break; // filesystem root
		dir = parent;
	}
	return null;
}

// ---------------------------------------------------------------------------
// Binary location detection (skill package)
// ---------------------------------------------------------------------------

/**
 * When running as a compiled binary at {skill}/.let/bin/let_*, detect the
 * .let/ directory by walking two levels up: bin/ -> .let/ -> {skill}/.
 * Validates that {skill}/SKILL.md exists. Returns the .let/ directory or null.
 *
 * Safety: non-compiled Bun's execPath points to ~/.bun/bin/bun (no .let parent),
 * and the monorepo bin/let binary's parent has package.json (no SKILL.md),
 * so detection falls through in both cases.
 */
function detectBinaryHome(): string | null {
	const binDir = dirname(process.execPath);
	if (basename(binDir) !== 'bin') return null;
	const dotLet = dirname(binDir);
	if (basename(dotLet) !== '.let') return null;
	const skillRoot = dirname(dotLet);
	if (existsSync(join(skillRoot, 'SKILL.md'))) return dotLet;
	return null;
}

// ---------------------------------------------------------------------------
// OS defaults
// ---------------------------------------------------------------------------

function linuxDefaults(): { config: string; data: string; cache: string; sources: string } {
	const home = homedir();
	const configHome = process.env['XDG_CONFIG_HOME'] || join(home, '.config');
	const dataHome = process.env['XDG_DATA_HOME'] || join(home, '.local', 'share');
	const cacheHome = process.env['XDG_CACHE_HOME'] || join(home, '.cache');
	return {
		config: join(configHome, 'let'),
		data: join(dataHome, 'let'),
		cache: join(cacheHome, 'let'),
		sources: join(dataHome, 'let', 'sources'),
	};
}

function darwinDefaults(): { config: string; data: string; cache: string; sources: string } {
	const home = homedir();
	const appSupport = join(home, 'Library', 'Application Support', 'let');
	return {
		config: appSupport,
		data: appSupport,
		cache: join(home, 'Library', 'Caches', 'let'),
		sources: join(appSupport, 'sources'),
	};
}

function osDefaults(): { config: string; data: string; cache: string; sources: string } {
	return platform() === 'darwin' ? darwinDefaults() : linuxDefaults();
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
	const configFileName = resolved.isDev ? 'let.config.toml' : 'config.toml';
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
 * Compute base defaults from dev detection, binary location, or OS conventions.
 */
function resolveDefaults(): { defaults: DirSet; isDev: boolean } {
	const monorepoRoot = detectMonorepoRoot(process.cwd());
	const isDev = monorepoRoot !== null;

	if (isDev && monorepoRoot) {
		return {
			defaults: {
				config: join(monorepoRoot, '.let', 'data'),
				data: join(monorepoRoot, '.let', 'data'),
				cache: join(monorepoRoot, '.let', 'cache'),
				sources: join(monorepoRoot, '.let', 'sources'),
			},
			isDev,
		};
	}

	const binaryHome = detectBinaryHome();
	if (binaryHome) {
		return {
			defaults: {
				config: join(binaryHome, 'data'),
				data: join(binaryHome, 'data'),
				cache: join(binaryHome, 'cache'),
				sources: join(binaryHome, 'sources'),
			},
			isDev: false,
		};
	}

	return { defaults: osDefaults(), isDev: false };
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

	const { defaults, isDev } = resolveDefaults();

	// Apply category env vars over defaults
	const resolved: ResolvedPaths = {
		config: envOrDefault('LET_CONFIG_DIR', defaults.config),
		data: envOrDefault('LET_DATA_DIR', defaults.data),
		cache: envOrDefault('LET_CACHE_DIR', defaults.cache),
		sources: envOrDefault('LET_SOURCES_DIR', defaults.sources),
		isDev,
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
