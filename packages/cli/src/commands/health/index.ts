/**
 * `health` — Prerequisites and system health check.
 *
 * Checks config, database, source DBs, API credentials, directory
 * permissions. All paths and fix commands use resolved paths().
 */

import { existsSync, mkdirSync, unlinkSync, writeFileSync } from 'node:fs';
import { loadListingsFile } from '@let/core/db';
import { paths } from '@let/core/paths';
import { log } from '@let/core/utils/logger';
import { defineCommand } from 'citty';
import { emitRaw, isJsonMode } from '../../envelope.js';

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

type CheckStatus = 'ok' | 'missing' | 'invalid' | 'outdated' | 'schema_mismatch';
type Severity = 'blocking' | 'degraded' | 'info';

interface HealthCheck {
	id: string;
	label: string;
	status: CheckStatus;
	severity: Severity;
	detail: string | null;
	fix: string[] | null;
}

// ---------------------------------------------------------------------------
// Source DB registry
// ---------------------------------------------------------------------------

const SOURCE_REGISTRY: Array<{ name: string; required: boolean; severity: Severity; buildCommand: string }> = [
	{ name: 'postcodes', required: true, severity: 'blocking', buildCommand: 'bun run build:source:postcodes' },
	{ name: 'broadband', required: false, severity: 'degraded', buildCommand: 'bun run build:source:broadband' },
	{ name: 'deprivation', required: false, severity: 'degraded', buildCommand: 'bun run build:source:deprivation' },
	{ name: 'census', required: false, severity: 'degraded', buildCommand: 'bun run build:source:census' },
	{ name: 'population', required: false, severity: 'degraded', buildCommand: 'bun run build:source:population' },
	{ name: 'income', required: false, severity: 'degraded', buildCommand: 'bun run build:source:income' },
	{ name: 'flood', required: false, severity: 'degraded', buildCommand: 'bun run build:source:flood' },
	{ name: 'crime', required: false, severity: 'degraded', buildCommand: 'bun run build:source:crime' },
	{ name: 'naptan', required: false, severity: 'degraded', buildCommand: 'bun run build:source:naptan' },
	{ name: 'uprn', required: false, severity: 'degraded', buildCommand: 'bun run build:source:uprn' },
];

// ---------------------------------------------------------------------------
// Individual checks
// ---------------------------------------------------------------------------

function checkConfig(p: ReturnType<typeof paths>): HealthCheck {
	const configFile = p.derived.configFile;
	if (!existsSync(configFile)) {
		return {
			id: 'config',
			label: 'Configuration',
			status: 'missing',
			severity: 'blocking',
			detail: configFile,
			fix: [`cp ${p.derived.templateFile} ${configFile}`],
		};
	}
	// Try loading config to validate
	try {
		const file = Bun.file(configFile);
		// Synchronous check: just verify it's parseable TOML
		if (file.size === 0) {
			return {
				id: 'config',
				label: 'Configuration',
				status: 'invalid',
				severity: 'blocking',
				detail: `${configFile} is empty`,
				fix: [`cp ${p.derived.templateFile} ${configFile}`],
			};
		}
		return {
			id: 'config',
			label: 'Configuration',
			status: 'ok',
			severity: 'info',
			detail: configFile,
			fix: null,
		};
	} catch {
		return {
			id: 'config',
			label: 'Configuration',
			status: 'invalid',
			severity: 'blocking',
			detail: configFile,
			fix: [`cp ${p.derived.templateFile} ${configFile}`],
		};
	}
}

function checkDatabase(p: ReturnType<typeof paths>): HealthCheck {
	const dbPath = p.derived.database;
	if (!existsSync(dbPath)) {
		return {
			id: 'database',
			label: 'Listings Database',
			status: 'missing',
			severity: 'info',
			detail: `${dbPath} (created on first fetch)`,
			fix: null,
		};
	}
	try {
		const data = loadListingsFile(dbPath);
		const count = data.listings?.length ?? 0;
		return {
			id: 'database',
			label: 'Listings Database',
			status: 'ok',
			severity: 'info',
			detail: `${dbPath} (${count} listings)`,
			fix: null,
		};
	} catch (e) {
		const msg = e instanceof Error ? e.message : String(e);
		if (msg.includes('no such column') || msg.includes('no such table')) {
			return {
				id: 'database',
				label: 'Listings Database',
				status: 'schema_mismatch',
				severity: 'blocking',
				detail: `${dbPath} — schema incompatible`,
				fix: [`cp ${p.derived.backup} ${dbPath}`, `# or delete ${dbPath} to start fresh`],
			};
		}
		return {
			id: 'database',
			label: 'Listings Database',
			status: 'invalid',
			severity: 'blocking',
			detail: `${dbPath} — ${msg}`,
			fix: [`cp ${p.derived.backup} ${dbPath}`],
		};
	}
}

function checkSource(p: ReturnType<typeof paths>, source: (typeof SOURCE_REGISTRY)[number]): HealthCheck {
	const dbPath = p.derived.sourceDb(source.name);
	if (!existsSync(dbPath)) {
		return {
			id: `source.${source.name}`,
			label: `Source: ${source.name}`,
			status: 'missing',
			severity: source.severity,
			detail: dbPath,
			fix: [source.buildCommand],
		};
	}
	return {
		id: `source.${source.name}`,
		label: `Source: ${source.name}`,
		status: 'ok',
		severity: 'info',
		detail: dbPath,
		fix: null,
	};
}

function checkEnvVar(p: ReturnType<typeof paths>, name: string, label: string): HealthCheck {
	const envFile = p.derived.envFile;
	const value = process.env[name];

	if (value) {
		return {
			id: `env.${name.toLowerCase()}`,
			label,
			status: 'ok',
			severity: 'info',
			detail: 'Set',
			fix: null,
		};
	}
	return {
		id: `env.${name.toLowerCase()}`,
		label,
		status: 'missing',
		severity: 'degraded',
		detail: `${name} not set`,
		fix: [`echo '${name}=your-key' >> ${envFile}`],
	};
}

function checkDirWritable(dirPath: string, label: string): HealthCheck {
	const id = `dir.${label.toLowerCase().replace(/\s+/g, '_')}`;
	const fullLabel = `Directory: ${label}`;
	try {
		if (!existsSync(dirPath)) {
			mkdirSync(dirPath, { recursive: true });
		}
		const testFile = `${dirPath}/.health-check-${Date.now()}`;
		writeFileSync(testFile, '');
		unlinkSync(testFile);
		return { id, label: fullLabel, status: 'ok', severity: 'info', detail: dirPath, fix: null };
	} catch {
		return { id, label: fullLabel, status: 'invalid', severity: 'blocking', detail: `${dirPath} — not writable`, fix: [`mkdir -p ${dirPath}`, `chmod 755 ${dirPath}`] };
	}
}

// ---------------------------------------------------------------------------
// Main health check
// ---------------------------------------------------------------------------

function runHealthChecks(): { checks: HealthCheck[]; status: 'ready' | 'degraded' | 'blocked' } {
	const p = paths();
	const checks: HealthCheck[] = [];

	// 1. Config
	checks.push(checkConfig(p));

	// 2. Database
	checks.push(checkDatabase(p));

	// 3-12. Source databases
	for (const source of SOURCE_REGISTRY) {
		checks.push(checkSource(p, source));
	}

	// 13-15. API credentials
	checks.push(checkEnvVar(p, 'EPC_API_KEY', 'EPC API Key'));
	checks.push(checkEnvVar(p, 'NOTION_API_KEY', 'Notion API Key'));
	checks.push(checkEnvVar(p, 'MAPBOX_ACCESS_TOKEN', 'Mapbox Token'));

	// 16-17. Directory permissions
	checks.push(checkDirWritable(p.resolved.data, 'data'));
	checks.push(checkDirWritable(p.resolved.cache, 'cache'));

	// Compute status
	const hasBlocking = checks.some((c) => c.severity === 'blocking' && c.status !== 'ok');
	const hasDegraded = checks.some((c) => c.severity === 'degraded' && c.status !== 'ok');
	const status = hasBlocking ? 'blocked' : hasDegraded ? 'degraded' : 'ready';

	return { checks, status };
}

// ---------------------------------------------------------------------------
// Summarize + output helpers
// ---------------------------------------------------------------------------

function computeSummary(checks: HealthCheck[]): { ok: number; blocking: number; degraded: number } {
	return {
		ok: checks.filter((c) => c.status === 'ok').length,
		blocking: checks.filter((c) => c.severity === 'blocking' && c.status !== 'ok').length,
		degraded: checks.filter((c) => c.severity === 'degraded' && c.status !== 'ok').length,
	};
}

function checkIcon(check: HealthCheck): string {
	if (check.status === 'ok') return '✓';
	if (check.severity === 'blocking') return '✗';
	return '!';
}

function printCheck(check: HealthCheck): void {
	const icon = checkIcon(check);
	const detail = check.detail ? ` (${check.detail})` : '';
	log.cli.info(`  ${icon} ${check.label}: ${check.status}${detail}`);
	if (check.fix && check.status !== 'ok') {
		for (const cmd of check.fix) {
			log.cli.info(`    Fix: ${cmd}`);
		}
	}
}

function statusLabel(status: string): string {
	if (status === 'ready') return 'READY';
	if (status === 'degraded') return 'DEGRADED';
	return 'BLOCKED';
}

function printTextOutput(p: ReturnType<typeof paths>, checks: HealthCheck[], status: string, summary: { ok: number; blocking: number; degraded: number }): void {
	log.cli.info(`Health: ${statusLabel(status)}`);
	log.cli.info(`Paths: config=${p.resolved.config} data=${p.resolved.data} cache=${p.resolved.cache} sources=${p.resolved.sources} isDev=${p.resolved.isDev}`);
	for (const check of checks) printCheck(check);
	log.cli.info(`Summary: ${summary.ok} ok, ${summary.blocking} blocking, ${summary.degraded} degraded`);
}

// ---------------------------------------------------------------------------
// Command
// ---------------------------------------------------------------------------

export const healthCommand = defineCommand({
	meta: {
		name: 'health',
		description: 'Check prerequisites and system health',
	},
	args: {
		json: {
			type: 'boolean' as const,
			description: 'Output as JSON envelope',
			default: false,
		},
	},
	run() {
		const start = performance.now();
		const jsonMode = isJsonMode();
		const p = paths();
		const { checks, status } = runHealthChecks();
		const summary = computeSummary(checks);

		if (jsonMode) {
			const data = {
				status,
				paths: { config: p.resolved.config, data: p.resolved.data, cache: p.resolved.cache, sources: p.resolved.sources, isDev: p.resolved.isDev },
				checks,
				summary,
			};
			const elapsed = Math.round(performance.now() - start);
			const envelope = { ok: true, data, meta: { tool: 'health', elapsed } };
			emitRaw(JSON.stringify(envelope), status === 'blocked' ? 2 : 0);
		}

		printTextOutput(p, checks, status, summary);
		if (status === 'blocked') process.exit(2);
	},
});
