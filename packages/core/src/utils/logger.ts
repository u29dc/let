/**
 * Structured logging utility
 *
 * Domain-based logging with consistent, dense formatting.
 */

import { color } from '@let/core/utils/terminal';

type LogLevel = 'debug' | 'info' | 'success' | 'warn' | 'error';
type Domain = 'CLI' | 'FETCH' | 'IMAGES_FETCH' | 'MAPS_FETCH' | 'PARSE' | 'ENRICH' | 'SCORE' | 'NOTION';

const LEVEL_LABELS: Record<LogLevel, string> = {
	debug: 'DEBUG',
	info: 'INFO',
	success: 'INFO',
	warn: 'WARN',
	error: 'ERROR',
};

const LEVEL_WIDTH = 5;
const DOMAIN_WIDTH = 12;
const TIME_WIDTH = 9;

let quietMode = false;

/**
 * Enable/disable quiet mode
 */
export function setQuietMode(quiet: boolean): void {
	quietMode = quiet;
}

/**
 * Create a domain-specific logger
 */
function createDomainLogger(domain: Domain) {
	const isProgressLog = (message: string, data?: unknown): boolean => {
		if (message.toLowerCase().includes('progress')) return true;
		if (data && typeof data === 'object') {
			const record = data as Record<string, unknown>;
			if (record['progress']) return true;
			if (typeof record['current'] === 'number' && typeof record['total'] === 'number') return true;
		}
		return false;
	};

	const formatTimestamp = (): string => new Date().toISOString().slice(11, 19);

	const formatStringValue = (value: string): string => {
		if (value.length === 0) return '""';
		if (/[\s=]/.test(value)) return JSON.stringify(value);
		return value;
	};

	const formatJsonValue = (value: unknown): string => {
		try {
			return JSON.stringify(value);
		} catch {
			return String(value);
		}
	};

	const formatValue = (value: unknown): string => {
		if (value === null) return 'null';
		if (value === undefined) return 'undefined';
		if (typeof value === 'string') return formatStringValue(value);
		if (typeof value === 'number' || typeof value === 'boolean') return String(value);
		return formatJsonValue(value);
	};

	const formatData = (data: unknown): string => {
		if (data === undefined) return '';
		if (data && typeof data === 'object' && !Array.isArray(data)) {
			const entries = Object.entries(data as Record<string, unknown>);
			return entries.map(([key, value]) => `${key}=${formatValue(value)}`).join(' ');
		}
		return `data=${formatValue(data)}`;
	};

	const styleLevel = (level: LogLevel, label: string): string => {
		if (level === 'debug') return color.dim(label);
		if (level === 'warn') return color.yellow(label);
		if (level === 'error') return color.red(label);
		return label;
	};

	const emit = (level: LogLevel, message: string, data?: unknown): void => {
		if (quietMode) return;
		const withTime = level === 'debug' || isProgressLog(message, data);
		const levelLabel = LEVEL_LABELS[level].padEnd(LEVEL_WIDTH);
		const domainLabel = domain.padEnd(DOMAIN_WIDTH);
		const timeLabel = withTime ? `${formatTimestamp()} ` : ' '.repeat(TIME_WIDTH);
		const prefix = `${timeLabel}${styleLevel(level, levelLabel)} ${domainLabel}`;
		const dataText = formatData(data);
		const line = dataText ? `${prefix} ${message} ${dataText}` : `${prefix} ${message}`;

		if (level === 'error' || level === 'warn') {
			// biome-ignore lint/suspicious/noConsole: CLI logging
			console.error(line);
			return;
		}
		// biome-ignore lint/suspicious/noConsole: CLI logging
		console.log(line);
	};

	return {
		debug: (message: string, data?: unknown) => emit('debug', message, data),
		info: (message: string, data?: unknown) => emit('info', message, data),
		success: (message: string, data?: unknown) => emit('success', message, data),
		warn: (message: string, data?: unknown) => emit('warn', message, data),
		error: (message: string, data?: unknown) => emit('error', message, data),
	};
}

/**
 * Domain-specific loggers aligned with pipeline stages
 *
 * Usage:
 *   log.cli.info("Processing listing", { id: "123" });
 *   log.fetch.debug("HTTP request", { url: "..." });
 *   log.parse.error("Failed to extract", { error: "..." });
 */
export const log = {
	/** CLI operations and user output */
	cli: createDomainLogger('CLI'),
	/** Stage 1: HTTP requests, rate limiting */
	fetch: createDomainLogger('FETCH'),
	/** Stage 1 (sub): Image downloads */
	fetchImages: createDomainLogger('IMAGES_FETCH'),
	/** Stage 1 (sub): Mapbox satellite + street */
	fetchMaps: createDomainLogger('MAPS_FETCH'),
	/** Stage 2: HTML/JSON extraction */
	parse: createDomainLogger('PARSE'),
	/** Stage 3: EPC, broadband, notes */
	enrich: createDomainLogger('ENRICH'),
	/** Stage 4: Scoring calculations */
	score: createDomainLogger('SCORE'),
	/** Output: Notion API export */
	notion: createDomainLogger('NOTION'),
};

export type { LogLevel, Domain };
