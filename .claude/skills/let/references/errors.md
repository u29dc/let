# Error Reference

Error codes returned in the JSON envelope `error.code` field when `ok: false`.

| Code               | Meaning                          | Recovery Action                                                             |
| ------------------ | -------------------------------- | --------------------------------------------------------------------------- |
| `NO_CONFIG`        | Config file not found            | Create from `templates/config.toml`. See `references/init.md`.              |
| `NO_SOURCES`       | Source databases missing         | Proceed degraded or build from repo. See `references/init.md`.              |
| `NO_DATABASE`      | Listings DB not found            | Normal on first run. Fetch creates the DB automatically.                    |
| `SCHEMA_MISMATCH`  | DB schema incompatible           | Delete the database file and re-fetch. Schema has changed.                  |
| `RATE_LIMITED`     | Too many requests to portal      | Wait 10-30 seconds, increase `--delay`, retry once. Do not spam.            |
| `NOT_FOUND`        | Listing removed from portal      | Skip this ID and continue with remaining listings.                          |
| `VALIDATION_ERROR` | Invalid input data               | Fix input according to the schema. Check `error.hint` for details.          |
| `API_ERROR`        | External API failure (EPC, etc.) | Log the error, skip affected enrichment step, continue with available data. |

## Exit codes

| Code | Meaning                                     |
| ---- | ------------------------------------------- |
| 0    | Success (including partial success)         |
| 1    | Runtime error                               |
| 2    | Prerequisites blocked (health check failed) |

## General recovery strategy

1. Read `error.code` and `error.hint` from the JSON envelope.
2. Apply the recovery action from the table above.
3. If the error recurs after recovery, check `bin/let health --json` for systemic issues.
4. For rate limiting, back off exponentially: 3s, 5s, 10s. Never retry more than twice.
5. For missing data (sources, EPC, maps), proceed with lower confidence rather than blocking the pipeline.
