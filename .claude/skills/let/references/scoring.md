# Scoring Reference

## Score interpretation

Scores are percentile-relative within the current database. The agent adds value by detecting what photos reveal, researching neighborhoods, and identifying tradeoffs the algorithm cannot weigh.

| Range  | Meaning                                        | Action         |
| ------ | ---------------------------------------------- | -------------- |
| 85-100 | Exceptional -- strong across all dimensions    | Must assess    |
| 70-84  | Good -- strong in most areas, minor weaknesses | Assess         |
| 55-69  | Average -- mixed, moderate penalties           | Assess if time |
| 40-54  | Below average -- significant weaknesses        | Skip usually   |
| < 40   | Poor -- major penalties or deal-breakers       | Skip           |

## Composites

Three composite scores are weighted and aggregated:

- **Affordability** (default 30%) -- true monthly cost (rent + estimated heating by EPC band), price percentile
- **Location** (default 40%) -- station proximity, broadband (gigabit %), region priority, IMD decile, crime rate
- **Liveability** (default 30%) -- garden type, heating type, property type

Penalties (`epc`, `garden`, `pets`) are applied multiplicatively after composite aggregation. A single penalty can dominate the final score.

## Override flags for `search discover`

| Flag                       | Example                           | Effect                                                      |
| -------------------------- | --------------------------------- | ----------------------------------------------------------- |
| `--location <ID>`          | `--location REGION^904`           | Search a non-config location (get ID from `search resolve`) |
| `--location-name <name>`   | `--location-name Manchester`      | Display name for ad-hoc location                            |
| `--property-types <list>`  | `--property-types flat,apartment` | Override property types                                     |
| `--must-have <list\|none>` | `--must-have none`                | Override mustHave filters ("none" clears)                   |
| `--dont-show <list\|none>` | `--dont-show none`                | Override dontShow filters ("none" clears)                   |
| `--limit <n>`              | `--limit 50`                      | Max results per location                                    |

When `--location` is used, `mustHave`, `dontShow`, and `propertyTypes` are automatically cleared unless explicitly passed.

## Score adjustment guidance

When submitting an assessment, you may adjust the algorithmic score by -30 to +30 points via `scoreAdjustment`.

| Adjustment | When to use                                                              |
| ---------- | ------------------------------------------------------------------------ |
| +15 to +30 | Photos reveal exceptional quality the algorithm cannot detect            |
| +1 to +14  | Minor positives: unusually good layout, recent renovation, quiet street  |
| 0          | Algorithm score seems fair given the evidence                            |
| -1 to -14  | Minor negatives: dated interior, small rooms, noisy road visible on maps |
| -15 to -30 | Major red flags: damp signs, missing rooms in photos, industrial area    |

Always explain the adjustment in 1-2 sentences in the `reasoning` field. If media was missing, note that and reduce confidence rather than guessing. Adjustments should be evidence-based, not speculative.
