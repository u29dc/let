# Assess Listings

Analyze property listings via photo/map analysis for qualitative evaluation. Read `CLAUDE.md` for schema and CLI reference.

## Orchestration

This command runs parallel subagents for batch assessment.

**Workflow**:

1. Run `bun run let assess --top N` to get unassessed listings
2. Partition IDs across 5-10 subagents (2-3 listings each)
3. Launch subagents in parallel via Task tool with `subagent_type=general-purpose`

**Conflict avoidance**: Each subagent gets disjoint IDs; JSON writes are per-listing atomic.

## Subagent Prompt Template

Use this prompt when launching each subagent (replace `{IDS}` with comma-separated listing IDs):

```
Assess listings: {IDS}

For each ID:
1. Glob `.cache/{id}/*.webp` then Read each image (property photos, satellite, street map)
2. Run `bun run let assess {id}` to view listing details with notes and scores
3. Analyze: maintenance quality, natural light, spaciousness, what photos show/hide, neighborhood from maps
4. Submit: `bun run let assess {id} --json '{"maintenance":"...","lightAndSpace":"...","photoAnalysis":"...","neighborhoodAnalysis":"...","recommendation":"...","familySuitability":"...","reasoning":"...","scoreAdjustment":0}'`

Assessment guidance:

MAINTENANCE (from photos)
- excellent: pristine, renovated, quality finishes
- good: well-maintained, clean, no issues
- fair: dated but functional, minor wear
- poor: neglected, damage, needs work

LIGHT/SPACE: window size, brightness, ceiling height, room proportions, layout flow

PHOTOS: missing rooms = red flag, awkward angles, wide-angle distortion, dark/edited, damage signs

NEIGHBORHOOD (maps show ~10min walk radius with red pin)
- Satellite: green space, density (terraced vs detached), industrial concerns, busy roads
- Street: park/school names, POIs, transport, confirm satellite observations

FAMILY: safe play areas, storage, school/park proximity, quiet indicators

RECOMMENDATION
- strong-recommend: exceptional, all criteria met
- recommend: good with minor compromises
- neutral: average
- avoid: significant issues

No coordination needed - your IDs are unique.
```

## Image Cache

| Aspect   | Detail                                                  |
| -------- | ------------------------------------------------------- |
| Location | `.cache/{id}/`                                          |
| Photos   | `{id}-photo-{hash}.webp`                                |
| Maps     | `{id}-satellite-{hash}.webp`, `{id}-street-{hash}.webp` |
| Format   | WebP 900-1200px                                         |
| Maps     | ~10min walk radius, red pin marks property              |

## Assessment Schema

| Field                | Type                                       | Description                              |
| -------------------- | ------------------------------------------ | ---------------------------------------- |
| maintenance          | `excellent/good/fair/poor`                 | property condition from photos           |
| lightAndSpace        | string                                     | natural light, spaciousness, layout      |
| photoAnalysis        | string                                     | coverage, what's shown/hidden, red flags |
| recommendation       | `strong-recommend/recommend/neutral/avoid` | overall assessment                       |
| familySuitability    | `excellent/good/fair/poor`                 | suitability for family                   |
| reasoning            | string                                     | explain recommendation                   |
| scoreAdjustment      | -30 to +30                                 | manual adjustment (use 0 for no change)  |
| neighborhoodAnalysis | string                                     | satellite+street findings (optional)     |
| tradeoffs            | string                                     | compensating factors (optional)          |

## Visual Assessment Quick Reference

**Property Condition**: walls/ceilings (cracks, stains), flooring (worn, scratched), fixtures (modern vs dated), windows, cleanliness

**Light/Space**: window size, brightness, ceiling height, room proportions, layout flow, south-facing orientation

**Photo Red Flags**: missing rooms, awkward angles, excessive wide-angle, dark/edited photos, clutter, visible damage/damp

**Neighborhood (maps)**: green space (parks, fields, tree coverage), density (terraced vs detached), concerns (industrial, busy roads, railways), walkability to amenities

**Family Criteria**: safe play areas, storage space, school/park proximity, quiet residential indicators, pushchair access

**Trade-off Examples**: "north garden but conservatory floods with light", "far from station but quiet cul-de-sac", "dated kitchen but excellent bones"

## Example

```bash
bun run let assess 170448131 --json '{"maintenance":"good","lightAndSpace":"bright bay window, spacious bedrooms, good ceiling heights","photoAnalysis":"all rooms shown, honest representation","neighborhoodAnalysis":"Meadow Park 5min east, quiet cul-de-sac, Greenfield Primary nearby","recommendation":"recommend","familySuitability":"good","reasoning":"well-maintained, good layout, park nearby","scoreAdjustment":5}'
```

## Output

After assessment: `assessment` object populated, `assessedAt` timestamp set, `assessedScore` = algorithm score + adjustment.

View updated rankings: `bun run let view list --top 20`
