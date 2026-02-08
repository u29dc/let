/**
 * `assess submit <id> <json>` — Submit validated assessment.
 *
 * Validates assessment JSON against schema, computes assessed score,
 * and persists to database.
 */

import { loadListingsFile, saveListingsFile as saveToDb } from '@let/core/db';
import { paths } from '@let/core/paths';
import { calculateAssessedScore, normalizeAssessment } from '@let/core/pipeline/assess';
import { findListingById } from '@let/core/pipeline/view';
import { AssessmentSchema } from '@let/core/schema';
import { log } from '@let/core/utils/logger';
import { emitRaw, fail, isJsonMode, ok, rethrowCapture } from '../../envelope.js';
import { defineToolCommand } from '../../tool.js';

function parseAssessmentJson(raw: string) {
	try {
		return { data: JSON.parse(raw), error: null };
	} catch (e) {
		return { data: null, error: e instanceof Error ? e.message : 'Invalid JSON' };
	}
}

function validateAssessment(data: unknown) {
	const result = AssessmentSchema.safeParse(data);
	if (result.success) {
		return { assessment: result.data, errors: null };
	}
	const errors = result.error.issues.map((issue) => ({
		path: issue.path.join('.'),
		message: issue.message,
	}));
	return { assessment: null, errors };
}

/** Handle parse error and exit */
function handleParseError(jsonMode: boolean, parseError: string, start: number): never {
	if (jsonMode) {
		fail('assess.submit', 'PARSE_ERROR', `Invalid JSON: ${parseError}`, 'Check JSON syntax', start);
	}
	log.cli.error(`Invalid JSON: ${parseError}`);
	process.exit(1);
}

/** Handle validation errors and exit */
function handleValidationErrors(jsonMode: boolean, errors: Array<{ path: string; message: string }>, start: number): never {
	if (jsonMode) {
		const envelope = { ok: true, data: { valid: false, errors }, meta: { tool: 'assess.submit', elapsed: Math.round(performance.now() - start) } };
		emitRaw(JSON.stringify(envelope), 1);
	}
	log.cli.error('Assessment validation failed:');
	for (const err of errors) {
		log.cli.error(`  ${err.path}: ${err.message}`);
	}
	process.exit(1);
}

/** Handle listing not found and exit */
function handleNotFound(jsonMode: boolean, id: string, start: number): never {
	if (jsonMode) {
		fail('assess.submit', 'NOT_FOUND', `Listing not found: ${id}`, 'Check ID with "let view list"', start);
	}
	log.cli.error(`Listing not found: ${id}`);
	process.exit(1);
}

export const assessSubmitCommand = defineToolCommand(
	{
		name: 'assess.submit',
		command: 'let assess submit',
		category: 'assess',
		outputFields: ['id', 'assessedScore', 'algoScore', 'scoreAdjustment'],
		idempotent: false,
		rateLimit: null,
		example: 'let assess submit 170448131 \'{"maintenance":"good",...}\' --json',
	},
	{
		meta: {
			name: 'submit',
			description: 'Submit assessment for a listing',
		},
		args: {
			id: {
				type: 'positional' as const,
				description: 'Listing UUID or portal ID',
				required: true,
			},
			assessment: {
				type: 'positional' as const,
				description: 'Assessment JSON string',
				required: true,
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
			const dbPath = p.derived.database;

			try {
				const { data: parsed, error: parseError } = parseAssessmentJson(args.assessment);
				if (parseError) handleParseError(jsonMode, parseError, start);

				const { assessment, errors } = validateAssessment(parsed);
				if (errors) handleValidationErrors(jsonMode, errors, start);

				const data = loadListingsFile(dbPath);
				const listings = data.listings ?? [];
				const listing = findListingById(listings, args.id);
				if (!listing) handleNotFound(jsonMode, args.id, start);

				const normalized = normalizeAssessment(assessment);
				listing.assessment = normalized;
				listing.assessedAt = new Date().toISOString();

				const algoScore = listing.scores?._overall ?? 0;
				listing.assessedScore = calculateAssessedScore(algoScore, normalized);

				saveToDb(dbPath, { ...data, updatedAt: new Date().toISOString(), listings });

				const result = {
					id: listing.id,
					assessedScore: listing.assessedScore,
					algoScore,
					scoreAdjustment: normalized.scoreAdjustment,
				};

				if (jsonMode) {
					ok('assess.submit', result, start);
				}

				log.cli.info(`Assessment saved for ${args.id}: assessed=${listing.assessedScore} (algo=${algoScore} + adj=${normalized.scoreAdjustment})`);
			} catch (error) {
				rethrowCapture(error);
				const message = error instanceof Error ? error.message : String(error);
				if (jsonMode) {
					fail('assess.submit', 'SUBMIT_ERROR', `Assessment failed: ${message}`, 'Check input and database', start);
				}
				log.cli.error(`Assessment failed: ${message}`);
				process.exit(1);
			}
		},
	},
);
