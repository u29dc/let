/**
 * `score compute` — Rescore all listings.
 *
 * Recomputes scores for all listings using current config.
 * Deterministic given same DB + config. Also recalculates assessed scores.
 */

import { loadConfig, resetConfigCache } from '@let/core/config';
import { loadListingsFile, saveListingsFile as saveToDb } from '@let/core/db';
import { paths } from '@let/core/paths';
import { recalcAssessedScores, scoreListingsWithConfig } from '@let/core/pipeline/score';
import type { ListingsFile } from '@let/core/schema';
import { log, setQuietMode } from '@let/core/utils/logger';
import { fail, isJsonMode, ok } from '../../envelope.js';
import { defineToolCommand } from '../../tool.js';

export const scoreComputeCommand = defineToolCommand(
	{
		name: 'score.compute',
		command: 'let score compute',
		category: 'score',
		outputFields: ['total', 'scored', 'avgScore', 'avgConfidence'],
		idempotent: true,
		rateLimit: null,
		example: 'let score compute --json',
	},
	{
		meta: {
			name: 'compute',
			description: 'Rescore all listings',
		},
		args: {
			json: {
				type: 'boolean' as const,
				description: 'Output as JSON envelope',
				default: false,
			},
		},
		async run() {
			const start = performance.now();
			const jsonMode = isJsonMode();
			if (jsonMode) setQuietMode(true);
			const p = paths();
			const dbPath = p.derived.database;

			try {
				resetConfigCache();
				const config = await loadConfig(p.derived.configFile);
				const data = loadListingsFile(dbPath);
				const listings = data.listings ?? [];

				if (listings.length === 0) {
					if (jsonMode) {
						ok('score.compute', { total: 0, scored: 0, avgScore: 0, avgConfidence: 0 }, start);
					}
					log.cli.warn('No listings to score');
					return;
				}

				const scored = scoreListingsWithConfig(listings, config as unknown as Record<string, unknown>);
				recalcAssessedScores(scored);

				const totalScore = scored.reduce((sum, l) => sum + (l.scores?._overall ?? 0), 0);
				const totalConf = scored.reduce((sum, l) => sum + (l.scores?.confidence ?? 0), 0);
				const stats = {
					total: scored.length,
					scored: scored.filter((l) => l.scores).length,
					avgScore: Math.round((totalScore / scored.length) * 10) / 10,
					avgConfidence: Math.round((totalConf / scored.length) * 100) / 100,
				};

				const output: ListingsFile = {
					updatedAt: new Date().toISOString(),
					searchUrls: data.searchUrls ?? [],
					locations: data.locations ?? [],
					lastSearchTotal: data.lastSearchTotal ?? 0,
					listings: scored,
				};
				saveToDb(dbPath, output);

				if (jsonMode) {
					ok('score.compute', stats, start);
				}

				log.cli.info(`Rescored ${stats.scored} listings, avg score: ${stats.avgScore}`);
			} catch (error) {
				const message = error instanceof Error ? error.message : String(error);
				if (jsonMode) {
					fail('score.compute', 'SCORE_ERROR', `Scoring failed: ${message}`, 'Check config and database', start);
				}
				log.cli.error(`Scoring failed: ${message}`);
				process.exit(1);
			}
		},
	},
);
