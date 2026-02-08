# Summarize Property Listings

Analyze scraped Rightmove listings and produce a standardized summary report with full property context, including AI assessment data where available.

## Context

Read `CLAUDE.md` for project architecture, CLI reference, and schema fields.

## Execution Steps

1. Run `let view list --top 20 --json` for top listings overview and aggregate data
2. For each region, run `let view list --region <region> --top 5 --json`
3. Run `let view detail <id> --json` for top 10 overall listings (get full property data)
4. Run `let score explain <id> --json` for score breakdowns of top properties
5. Synthesize patterns and produce report

## Output Format (follow strictly)

### Section 1: Overview

- Total listings, regions covered
- Score distribution (excellent 80+, good 60-79, average 40-59, poor <40)
- Price range (min/max/median)
- Bedroom split (2-bed vs 3-bed counts)
- Assessment coverage: X of Y listings AI-assessed

### Section 2: Top Properties (detailed table)

| Rank | Property | Region | Price | Beds | Size | EPC | Broadband | Garden | Algo | Assessed | AI Rec | Notes |
| ---- | -------- | ------ | ----- | ---- | ---- | --- | --------- | ------ | ---- | -------- | ------ | ----- |

Column definitions:

- **Property**: Linked address `[Address](https://www.rightmove.co.uk/properties/{id})`
- **Size**: Floor area in sqm (or "-")
- **EPC**: Energy rating A-G (or "-")
- **Broadband**: Gigabit availability percent (or "-")
- **Garden**: "Yes", "No", or garden type from notes (e.g., "South 40ft")
- **Algo**: Algorithm score (0-100)
- **Assessed**: AI-assessed score (or "-" if not assessed)
- **AI Rec**: AI recommendation short form: "SR" (strong-recommend), "R" (recommend), "N" (neutral), "A" (avoid), "-" (not assessed)
- **Notes**: 2-3 key highlights from AI assessment or enrichment notes

Limit to top 10. Prioritize assessed listings. Show enough context to make viewing decisions.

### Section 3: Region Comparison (detailed table)

| Region | Count | Avg Algo | Avg Assessed | Best Value | EPC Trend | Broadband | Verdict |
| ------ | ----- | -------- | ------------ | ---------- | --------- | --------- | ------- |

Column definitions:

- **Avg Algo**: Average algorithm score
- **Avg Assessed**: Average AI-assessed score (or "-" if no assessments)
- **Best Value**: Link to top property in that region (prefer assessed if available)
- **EPC Trend**: Dominant rating (e.g., "Mostly C/D")
- **Broadband**: Typical gigabit availability (e.g., "80%", "Mixed")
- **Verdict**: 1-2 word assessment ("Best value", "Premium", "Overpriced", "Limited stock", "Avoid")

Rank by avg assessed score (if available) or avg algo score descending.

### Section 4: Property Deep Dives (top 5 only)

For each of the top 5 properties, provide a compact profile:

```
**#1: [Address](link)** - Price/mo, X bed, Xsqm
- EPC X | Broadband X% | X.Xmi to [Station]
- Garden: [details]
- Highlights: [from notes/description]
- Score breakdown: Algo X, Assessed X, Adjustment +/-Y
- AI Assessment: [maintenance], [recommendation], [key reasoning]
- Why it ranks: [1 sentence on what makes it stand out]
```

If not AI-assessed, note: "Not yet AI-assessed - recommend running `let assess`"

### Section 5: Pattern Analysis

Identify and explain 4-6 patterns with evidence:

- **Algo vs Assessed correlation**: Do AI assessments validate or challenge algorithm rankings?
- **Price-to-score correlation**: Which price bands score best?
- **EPC impact**: How much do ratings affect overall score?
- **Geographic trends**: Best value locations vs premium areas
- **Size availability**: Are larger properties scoring well or scarce?
- **Assessment insights**: Common themes from AI assessments (maintenance quality, photo honesty)
- **Anomalies**: Any outliers (cheap but low-scoring, expensive but high-scoring, big algo/assessed gaps)?

Use specific examples from the data.

### Section 6: Recommendations

**Must-view** (top 3):
For each, include: link, price, key selling points, any concerns, AI assessment summary if available

**Watch list** (2-3):
Properties worth monitoring - explain why (new listing, price may drop, awaiting assessment, etc.)

**Needs assessment** (up to 3):
High-scoring algorithm properties not yet AI-assessed - recommend running `let assess`

**Skip**:
Regions or property types underperforming - explain why, reference AI assessment insights if available

**Search refinements**:
Suggestions for adjusting config.toml based on findings (e.g., "Consider adding York - higher priority score", "Folkestone showing poor value at current price ceiling")

## Style

- Use ultrathink reasoning for pattern detection
- Be direct and data-driven
- Include Rightmove links for all mentioned properties
- Extract meaningful notes, not marketing fluff
- Show enough detail to make informed viewing decisions
- Highlight where AI assessment adds value vs algorithm-only ranking
