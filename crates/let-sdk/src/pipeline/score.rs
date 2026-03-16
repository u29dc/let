#![forbid(unsafe_code)]

use std::cmp::Ordering;

use sha2::{Digest, Sha256};

use crate::config::{AffordabilityConfig, AppConfig, HeatingCosts, PenaltyConfig, ScoringConfig};
use crate::schema::listing::{
    EpcBand, GardenType, HeatingType, Listing, ListingAssessment, PetPolicy, ScoreContext,
    ScoreFactors, ScorePenalties, ScorePercentiles, Scores, StatsSummary,
};

#[derive(Debug, Clone)]
struct PercentileContext {
    prices: Vec<f64>,
    true_costs: Vec<f64>,
    floor_areas: Vec<f64>,
    station_distances: Vec<f64>,
    crime_rates: Vec<f64>,
}

#[derive(Debug, Clone)]
struct RawFactors {
    monthly_rent: f64,
    floor_area_sqm: Option<f64>,
    epc_band: Option<EpcBand>,
    bedrooms: i64,
    station_miles: Option<f64>,
    gigabit_pct: Option<f64>,
    region_name: Option<String>,
    garden_type: GardenType,
    heating_type: HeatingType,
    pet_policy: PetPolicy,
    property_type: Option<String>,
    imd_decile: Option<i64>,
    crime_rate_per_1k: Option<f64>,
}

#[derive(Debug, Clone)]
struct NormalizedFactors {
    monthly_rent: f64,
    price_percentile: f64,
    floor_area_sqm: Option<f64>,
    floor_area_percentile: Option<f64>,
    epc_band: Option<EpcBand>,
    epc_numeric: Option<f64>,
    true_monthly_cost: f64,
    true_cost_percentile: f64,
    station_miles: Option<f64>,
    station_percentile: Option<f64>,
    gigabit_pct: Option<f64>,
    region_name: Option<String>,
    priority_score: Option<f64>,
    garden_type: GardenType,
    heating_type: HeatingType,
    pet_policy: PetPolicy,
    property_type: Option<String>,
    bedrooms: i64,
    imd_decile: Option<i64>,
    crime_rate_per_1k: Option<f64>,
    crime_rate_percentile: Option<f64>,
}

pub fn score_listings_with_config(listings: &[Listing], app_config: &AppConfig) -> Vec<Listing> {
    if listings.is_empty() {
        return Vec::new();
    }

    let percentiles = build_percentile_context(listings, &app_config.scoring);
    let stats = build_score_percentiles(&percentiles);
    let config_hash = scoring_config_hash(&app_config.scoring);

    let mut scored = listings
        .iter()
        .map(|listing| {
            score_single_listing(
                listing,
                &app_config.scoring,
                &percentiles,
                &stats,
                &config_hash,
            )
        })
        .collect::<Vec<_>>();

    scored.sort_by(|a, b| {
        let a_score = a.scores.as_ref().map_or(0.0, |scores| scores.overall);
        let b_score = b.scores.as_ref().map_or(0.0, |scores| scores.overall);
        b_score.partial_cmp(&a_score).unwrap_or(Ordering::Equal)
    });

    scored
}

pub fn recalc_assessed_scores(listings: &mut [Listing]) {
    for listing in listings.iter_mut() {
        if let (Some(scores), Some(assessment)) = (&listing.scores, &listing.assessment) {
            listing.assessed_score = Some(calculate_assessed_score(scores.overall, assessment));
        }
    }
}

pub fn calculate_assessed_score(algo_score: f64, assessment: &ListingAssessment) -> f64 {
    clamp(algo_score + assessment.score_adjustment, 0.0, 100.0)
}

fn score_single_listing(
    listing: &Listing,
    config: &ScoringConfig,
    percentiles: &PercentileContext,
    stats: &ScorePercentiles,
    config_hash: &str,
) -> Listing {
    let raw = extract_raw_factors(listing, config);
    let normalized = normalize_factors(raw, percentiles, config);

    let affordability = calculate_affordability(&normalized, &config.affordability);
    let location = calculate_location(&normalized, config);
    let liveability = calculate_liveability(&normalized, config);

    let penalties = calculate_penalties(&normalized, &config.penalties);
    let confidence = calculate_confidence(&normalized, config);

    let overall = aggregate_scores(
        affordability,
        location,
        liveability,
        config,
        penalties.combined,
    );

    let factors = ScoreFactors {
        monthly_rent: normalized.monthly_rent,
        price_percentile: round_to(normalized.price_percentile, 1),
        floor_area_sqm: normalized.floor_area_sqm,
        floor_area_percentile: normalized.floor_area_percentile.map(|v| round_to(v, 1)),
        epc_band: normalized.epc_band.as_ref().map(epc_band_str),
        epc_numeric: normalized.epc_numeric,
        true_monthly_cost: round_to(normalized.true_monthly_cost, 0),
        true_cost_percentile: round_to(normalized.true_cost_percentile, 1),
        station_miles: normalized.station_miles,
        station_percentile: normalized.station_percentile.map(|v| round_to(v, 1)),
        gigabit_pct: normalized.gigabit_pct,
        region_name: normalized.region_name,
        priority_score: normalized.priority_score,
        garden_type: normalized.garden_type,
        heating_type: normalized.heating_type,
        pet_policy: normalized.pet_policy,
        property_type: normalized.property_type,
        bedrooms: normalized.bedrooms,
        imd_decile: normalized.imd_decile,
        crime_rate_per_1k: normalized.crime_rate_per_1k,
        crime_rate_percentile: normalized.crime_rate_percentile,
    };

    let scores = Scores {
        overall,
        confidence: round_to(confidence, 2),
        affordability: round_to(affordability * 100.0, 0),
        location: round_to(location * 100.0, 0),
        liveability: round_to(liveability * 100.0, 0),
        factors,
        penalties,
        context: ScoreContext {
            config_hash: config_hash.to_owned(),
            percentiles: stats.clone(),
        },
    };

    let mut next = listing.clone();
    next.scores = Some(scores);
    if let Some(assessment) = &next.assessment {
        next.assessed_score = Some(calculate_assessed_score(overall, assessment));
    }
    next
}

fn build_percentile_context(listings: &[Listing], config: &ScoringConfig) -> PercentileContext {
    let mut prices = listings
        .iter()
        .map(|listing| listing.price as f64)
        .collect::<Vec<_>>();
    prices.sort_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal));

    let mut true_costs = listings
        .iter()
        .map(|listing| {
            listing.price as f64
                + get_heating_cost(
                    listing.epc_rating.as_ref(),
                    &config.affordability.heating_costs,
                )
        })
        .collect::<Vec<_>>();
    true_costs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal));

    let mut floor_areas = listings
        .iter()
        .filter_map(|listing| listing.floor_area_sqm)
        .collect::<Vec<_>>();
    floor_areas.sort_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal));

    let mut station_distances = listings
        .iter()
        .filter_map(nearest_station_distance)
        .collect::<Vec<_>>();
    station_distances.sort_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal));

    let mut crime_rates = listings
        .iter()
        .filter_map(|listing| listing.area.crime.rate_per_1k)
        .filter(|value| value.is_finite())
        .collect::<Vec<_>>();
    crime_rates.sort_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal));

    PercentileContext {
        prices,
        true_costs,
        floor_areas,
        station_distances,
        crime_rates,
    }
}

fn build_score_percentiles(percentiles: &PercentileContext) -> ScorePercentiles {
    ScorePercentiles {
        prices: calculate_stats(&percentiles.prices),
        true_costs: calculate_stats(&percentiles.true_costs),
        floor_areas: calculate_stats(&percentiles.floor_areas),
        station_distances: calculate_stats(&percentiles.station_distances),
        crime_rates: calculate_stats(&percentiles.crime_rates),
    }
}

fn scoring_config_hash(config: &ScoringConfig) -> String {
    let bytes = serde_json::to_vec(config).unwrap_or_default();
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

fn extract_raw_factors(listing: &Listing, config: &ScoringConfig) -> RawFactors {
    let region_name = extract_region_name(listing, config);

    RawFactors {
        monthly_rent: listing.price as f64,
        floor_area_sqm: listing.floor_area_sqm,
        epc_band: listing.epc_rating.clone(),
        bedrooms: listing.bedrooms,
        station_miles: nearest_station_distance(listing),
        gigabit_pct: listing.gigabit_availability,
        region_name,
        garden_type: detect_garden_type(listing),
        heating_type: detect_heating_type(listing),
        pet_policy: detect_pet_policy(listing),
        property_type: normalize_property_type(Some(listing.property_type.as_str())),
        imd_decile: listing.area.imd.decile,
        crime_rate_per_1k: listing.area.crime.rate_per_1k,
    }
}

fn normalize_factors(
    raw: RawFactors,
    percentiles: &PercentileContext,
    config: &ScoringConfig,
) -> NormalizedFactors {
    let heating_cost = get_heating_cost(raw.epc_band.as_ref(), &config.affordability.heating_costs);
    let true_monthly_cost = raw.monthly_rent + heating_cost;

    let price_percentile = calculate_percentile(raw.monthly_rent, &percentiles.prices, true);
    let true_cost_percentile =
        calculate_percentile(true_monthly_cost, &percentiles.true_costs, true);

    let floor_area_percentile = raw
        .floor_area_sqm
        .map(|value| calculate_percentile(value, &percentiles.floor_areas, false));

    let station_percentile = raw
        .station_miles
        .map(|value| calculate_percentile(value, &percentiles.station_distances, true));

    let crime_rate_percentile = raw
        .crime_rate_per_1k
        .map(|value| calculate_percentile(value, &percentiles.crime_rates, true));

    let priority_score = raw
        .region_name
        .as_ref()
        .and_then(|name| match_region_name(name, &config.region_priority))
        .and_then(|name| config.region_priority.get(&name).copied());

    let epc_band = raw.epc_band.clone();
    let epc_numeric = epc_band_to_numeric(epc_band.as_ref());

    NormalizedFactors {
        monthly_rent: raw.monthly_rent,
        price_percentile,
        floor_area_sqm: raw.floor_area_sqm,
        floor_area_percentile,
        epc_band,
        epc_numeric,
        true_monthly_cost,
        true_cost_percentile,
        station_miles: raw.station_miles,
        station_percentile,
        gigabit_pct: raw.gigabit_pct,
        region_name: raw.region_name,
        priority_score,
        garden_type: raw.garden_type,
        heating_type: raw.heating_type,
        pet_policy: raw.pet_policy,
        property_type: raw.property_type,
        bedrooms: raw.bedrooms,
        imd_decile: raw.imd_decile,
        crime_rate_per_1k: raw.crime_rate_per_1k,
        crime_rate_percentile,
    }
}

fn calculate_affordability(factors: &NormalizedFactors, config: &AffordabilityConfig) -> f64 {
    let true_cost_score = factors.true_cost_percentile / 100.0;
    let epc_score = factors
        .epc_numeric
        .map(|value| value / 100.0)
        .unwrap_or(0.5);
    weighted_arithmetic_mean(&[
        (true_cost_score, config.price_weight),
        (epc_score, config.epc_weight),
    ])
}

fn calculate_location(factors: &NormalizedFactors, config: &ScoringConfig) -> f64 {
    let station_score = factors
        .station_miles
        .map(station_proximity_utility)
        .unwrap_or(0.5);

    let broadband_score = factors.gigabit_pct.map(broadband_utility).unwrap_or(0.5);

    let priority_score = factors.priority_score.map(|value| value / 100.0);
    let imd_score = factors.imd_decile.map(imd_decile_to_score);
    let crime_score = factors.crime_rate_percentile.map(|value| value / 100.0);

    weighted_arithmetic_mean(&[
        (station_score, config.location.station_weight),
        (broadband_score, config.location.broadband_weight),
        (
            priority_score.unwrap_or(0.0),
            if priority_score.is_some() {
                config.location.priority_weight
            } else {
                0.0
            },
        ),
        (
            imd_score.unwrap_or(0.0),
            if imd_score.is_some() {
                config.location.imd_weight
            } else {
                0.0
            },
        ),
        (
            crime_score.unwrap_or(0.0),
            if crime_score.is_some() {
                config.location.crime_weight
            } else {
                0.0
            },
        ),
    ])
}

fn calculate_liveability(factors: &NormalizedFactors, config: &ScoringConfig) -> f64 {
    let garden_score = match factors.garden_type {
        GardenType::Private => config.liveability.garden.private,
        GardenType::Shared => config.liveability.garden.shared,
        GardenType::None => config.liveability.garden.none,
    } / 100.0;

    let heating_score = match factors.heating_type {
        HeatingType::Gas => config.liveability.heating.gas,
        HeatingType::Electric => config.liveability.heating.electric,
        HeatingType::Unknown => config.liveability.heating.unknown,
    } / 100.0;

    let property_type_score = property_type_score(factors.property_type.as_deref(), config);

    weighted_arithmetic_mean(&[
        (garden_score, config.liveability.garden_weight),
        (heating_score, config.liveability.heating_weight),
        (property_type_score, config.liveability.property_type_weight),
    ])
}

fn calculate_penalties(factors: &NormalizedFactors, config: &PenaltyConfig) -> ScorePenalties {
    let epc = match factors.epc_band {
        Some(EpcBand::G) => config.epc_g,
        Some(EpcBand::F) => config.epc_f,
        _ => 1.0,
    };

    let garden = if config.garden_required && matches!(factors.garden_type, GardenType::None) {
        config.no_garden
    } else {
        1.0
    };

    let pets = if matches!(factors.pet_policy, PetPolicy::No) {
        config.no_pets
    } else {
        1.0
    };

    let deprivation = if factors
        .imd_decile
        .is_some_and(|value| value <= config.deprivation_threshold)
    {
        config.deprivation
    } else {
        1.0
    };

    let high_crime = if factors
        .crime_rate_per_1k
        .is_some_and(|value| value > config.high_crime_threshold)
    {
        config.high_crime
    } else {
        1.0
    };

    let missing_data = calculate_missing_data_penalty(factors, config.missing_data_penalty);
    let combined = epc * garden * pets * deprivation * high_crime * missing_data;

    ScorePenalties {
        epc,
        garden,
        pets,
        combined,
    }
}

fn calculate_missing_data_penalty(factors: &NormalizedFactors, penalty: f64) -> f64 {
    if !penalty.is_finite() || penalty >= 1.0 {
        return 1.0;
    }

    let mut missing = 0;
    if factors.epc_band.is_none() {
        missing += 1;
    }
    if factors.station_miles.is_none() {
        missing += 1;
    }
    if factors.gigabit_pct.is_none() {
        missing += 1;
    }
    if factors.priority_score.is_none() {
        missing += 1;
    }
    if factors.imd_decile.is_none() {
        missing += 1;
    }
    if factors.crime_rate_per_1k.is_none() {
        missing += 1;
    }

    if missing == 0 {
        return 1.0;
    }

    penalty.powi(missing)
}

fn calculate_confidence(factors: &NormalizedFactors, config: &ScoringConfig) -> f64 {
    let weights = [
        (
            factors.epc_band.is_some(),
            config.weights.affordability * config.affordability.epc_weight,
            1.0,
        ),
        (
            true,
            config.weights.affordability * config.affordability.price_weight,
            1.0,
        ),
        (
            factors.station_miles.is_some(),
            config.weights.location * config.location.station_weight,
            1.0,
        ),
        (
            factors.gigabit_pct.is_some(),
            config.weights.location * config.location.broadband_weight,
            1.0,
        ),
        (
            factors.priority_score.is_some(),
            config.weights.location * config.location.priority_weight,
            1.0,
        ),
        (
            factors.imd_decile.is_some(),
            config.weights.location * config.location.imd_weight,
            1.0,
        ),
        (
            factors.crime_rate_per_1k.is_some(),
            config.weights.location * config.location.crime_weight,
            1.0,
        ),
        (
            true,
            config.weights.liveability * config.liveability.garden_weight,
            if matches!(factors.garden_type, GardenType::None) {
                0.5
            } else {
                1.0
            },
        ),
        (
            true,
            config.weights.liveability * config.liveability.heating_weight,
            if matches!(factors.heating_type, HeatingType::Unknown) {
                0.5
            } else {
                1.0
            },
        ),
        (
            factors.property_type.is_some(),
            config.weights.liveability * config.liveability.property_type_weight,
            1.0,
        ),
    ];

    let max_weight = weights.iter().map(|(_, weight, _)| *weight).sum::<f64>();
    if max_weight <= 0.0 {
        return 0.0;
    }

    let achieved = weights
        .iter()
        .map(|(present, weight, credit)| if *present { weight * credit } else { 0.0 })
        .sum::<f64>();

    achieved / max_weight
}

fn aggregate_scores(
    affordability: f64,
    location: f64,
    liveability: f64,
    config: &ScoringConfig,
    combined_penalty: f64,
) -> f64 {
    let weights = normalize_composite_weights(
        config.weights.affordability,
        config.weights.location,
        config.weights.liveability,
    );

    let values = [
        (affordability, weights.0),
        (location, weights.1),
        (liveability, weights.2),
    ];

    let raw = variance_adaptive_aggregate(
        &values,
        config.adaptiveness,
        0.3,
        config.adaptiveness_factor,
    );

    let penalized = raw * combined_penalty;
    clamp((penalized * 100.0).round(), 0.0, 100.0)
}

fn normalize_composite_weights(
    affordability: f64,
    location: f64,
    liveability: f64,
) -> (f64, f64, f64) {
    let safe_affordability = affordability.max(0.0);
    let safe_location = location.max(0.0);
    let safe_liveability = liveability.max(0.0);
    let total = safe_affordability + safe_location + safe_liveability;

    if !total.is_finite() || total <= 0.0 {
        return (1.0 / 3.0, 1.0 / 3.0, 1.0 / 3.0);
    }

    (
        safe_affordability / total,
        safe_location / total,
        safe_liveability / total,
    )
}

fn weighted_arithmetic_mean(values: &[(f64, f64)]) -> f64 {
    let non_zero = values
        .iter()
        .copied()
        .filter(|(_, weight)| *weight > 0.0)
        .collect::<Vec<_>>();

    if non_zero.is_empty() {
        return 0.0;
    }

    let total_weight = non_zero.iter().map(|(_, weight)| *weight).sum::<f64>();
    let weighted_sum = non_zero
        .iter()
        .map(|(value, weight)| value * weight)
        .sum::<f64>();

    weighted_sum / total_weight
}

fn weighted_geometric_mean(values: &[(f64, f64)]) -> f64 {
    let non_zero = values
        .iter()
        .copied()
        .filter(|(_, weight)| *weight > 0.0)
        .collect::<Vec<_>>();

    if non_zero.is_empty() {
        return 0.0;
    }

    let total_weight = non_zero.iter().map(|(_, weight)| *weight).sum::<f64>();
    let mut log_sum = 0.0;

    for (value, weight) in non_zero {
        let bounded = value.max(0.001);
        log_sum += (weight / total_weight) * bounded.ln();
    }

    log_sum.exp()
}

fn variance_adaptive_aggregate(
    values: &[(f64, f64)],
    adaptiveness: f64,
    center: f64,
    adaptiveness_factor: f64,
) -> f64 {
    let geo = weighted_geometric_mean(values);
    let arith = weighted_arithmetic_mean(values);

    let scores = values
        .iter()
        .copied()
        .filter(|(_, weight)| *weight > 0.0)
        .map(|(value, _)| value)
        .collect::<Vec<_>>();

    if scores.is_empty() {
        return 0.0;
    }

    let mean = scores.iter().sum::<f64>() / scores.len() as f64;
    if mean == 0.0 {
        return 0.0;
    }

    let variance = scores
        .iter()
        .map(|score| (score - mean).powi(2))
        .sum::<f64>()
        / scores.len() as f64;
    let std_dev = variance.sqrt();
    let cv = std_dev / mean;

    let alpha = sigmoid((cv - center) * adaptiveness * adaptiveness_factor);
    alpha * arith + (1.0 - alpha) * geo
}

fn calculate_percentile(value: f64, sorted: &[f64], invert: bool) -> f64 {
    if sorted.is_empty() {
        return 50.0;
    }

    if sorted.len() == 1 {
        let single = sorted[0];
        if value == single {
            return 50.0;
        }
        let better = if invert {
            value < single
        } else {
            value > single
        };
        return if better { 75.0 } else { 25.0 };
    }

    if sorted.len() == 2 {
        let first = sorted[0];
        let second = sorted[1];
        if first == second {
            return 50.0;
        }
        if value <= first {
            return if invert { 100.0 } else { 0.0 };
        }
        if value >= second {
            return if invert { 0.0 } else { 100.0 };
        }
        return 50.0;
    }

    let lower = lower_bound(sorted, value);
    let upper = upper_bound(sorted, value);
    let percentile = if lower == upper {
        (lower as f64 / sorted.len() as f64) * 100.0
    } else {
        ((lower as f64 + upper as f64) / 2.0 / sorted.len() as f64) * 100.0
    };

    if invert {
        100.0 - percentile
    } else {
        percentile
    }
}

fn lower_bound(sorted: &[f64], value: f64) -> usize {
    let mut low = 0usize;
    let mut high = sorted.len();
    while low < high {
        let mid = (low + high) / 2;
        if sorted[mid] < value {
            low = mid + 1;
        } else {
            high = mid;
        }
    }
    low
}

fn upper_bound(sorted: &[f64], value: f64) -> usize {
    let mut low = 0usize;
    let mut high = sorted.len();
    while low < high {
        let mid = (low + high) / 2;
        if sorted[mid] <= value {
            low = mid + 1;
        } else {
            high = mid;
        }
    }
    low
}

fn calculate_stats(values: &[f64]) -> StatsSummary {
    if values.is_empty() {
        return StatsSummary {
            min: 0.0,
            max: 0.0,
            mean: 0.0,
            median: 0.0,
            std_dev: 0.0,
        };
    }

    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal));

    let min = sorted[0];
    let max = *sorted.last().unwrap_or(&0.0);
    let mean = sorted.iter().sum::<f64>() / sorted.len() as f64;
    let mid = sorted.len() / 2;
    let median = if sorted.len().is_multiple_of(2) {
        (sorted[mid - 1] + sorted[mid]) / 2.0
    } else {
        sorted[mid]
    };

    let variance = sorted
        .iter()
        .map(|value| (value - mean).powi(2))
        .sum::<f64>()
        / sorted.len() as f64;

    StatsSummary {
        min,
        max,
        mean,
        median,
        std_dev: variance.sqrt(),
    }
}

fn detect_garden_type(listing: &Listing) -> GardenType {
    let text = listing_text(listing);

    if contains_phrase(&text, &["no garden"]) {
        return GardenType::None;
    }
    if contains_phrase(&text, &["shared garden", "communal garden"]) {
        return GardenType::Shared;
    }
    if contains_phrase(
        &text,
        &[
            "private garden",
            "enclosed garden",
            "rear garden",
            "front garden",
            "south-facing garden",
            "back garden",
            "west-facing garden",
            "large garden",
            "mature garden",
        ],
    ) {
        return GardenType::Private;
    }
    if contains_phrase(&text, &["garden"]) {
        return GardenType::Private;
    }
    if contains_phrase(
        &text,
        &["patio", "courtyard", "outside space", "outdoor space"],
    ) {
        return GardenType::Shared;
    }

    GardenType::None
}

fn detect_heating_type(listing: &Listing) -> HeatingType {
    let text = listing_text(listing);

    if contains_phrase(
        &text,
        &[
            "gas central heating",
            "gas heating",
            "gas ch",
            "gas fired",
            "gas boiler",
            "combi boiler",
        ],
    ) {
        return HeatingType::Gas;
    }

    if contains_phrase(
        &text,
        &[
            "electric heating",
            "storage heater",
            "storage heaters",
            "electric radiator",
            "electric radiators",
            "no gas",
            "all electric",
        ],
    ) {
        return HeatingType::Electric;
    }

    if text.contains("central heating") && !text.contains("no gas") {
        return HeatingType::Gas;
    }

    HeatingType::Unknown
}

fn detect_pet_policy(listing: &Listing) -> PetPolicy {
    let text = listing_text(listing);

    if contains_phrase(
        &text,
        &[
            "pets allowed",
            "pets welcome",
            "pets considered",
            "pets friendly",
            "pets negotiable",
            "pet-friendly",
        ],
    ) {
        return PetPolicy::Yes;
    }

    if contains_phrase(&text, &["no pets", "pets not allowed", "sorry no pets"]) {
        return PetPolicy::No;
    }

    PetPolicy::Unknown
}

fn listing_text(listing: &Listing) -> String {
    let mut text = String::with_capacity(listing.description.len() + 256);
    text.push_str(&listing.description);
    for note in &listing.notes {
        text.push(' ');
        text.push_str(note);
    }
    text.to_lowercase()
}

fn contains_phrase(text: &str, phrases: &[&str]) -> bool {
    phrases
        .iter()
        .any(|phrase| contains_word_bounded_phrase(text, phrase))
}

fn contains_word_bounded_phrase(text: &str, phrase: &str) -> bool {
    if phrase.is_empty() || text.len() < phrase.len() {
        return false;
    }

    let mut offset = 0usize;
    while let Some(found) = text[offset..].find(phrase) {
        let start = offset + found;
        let end = start + phrase.len();

        let left_is_boundary = text[..start]
            .chars()
            .next_back()
            .is_none_or(|ch| !ch.is_ascii_alphanumeric());
        let right_is_boundary = text[end..]
            .chars()
            .next()
            .is_none_or(|ch| !ch.is_ascii_alphanumeric());

        if left_is_boundary && right_is_boundary {
            return true;
        }

        offset = start.saturating_add(1);
        if offset >= text.len() {
            break;
        }
    }

    false
}

fn nearest_station_distance(listing: &Listing) -> Option<f64> {
    listing
        .nearest_stations
        .first()
        .map(|station| station.distance)
}

fn normalize_property_type(value: Option<&str>) -> Option<String> {
    let raw = value?.trim().to_lowercase();
    if raw.is_empty() {
        return None;
    }

    let normalized = if raw.contains("semi-detached")
        || raw.contains("semi detached")
        || raw.contains("semidetached")
        || raw == "semi"
    {
        "semi-detached"
    } else if raw.contains("detached") {
        "detached"
    } else if raw.contains("terraced")
        || raw.contains("terrace")
        || raw.contains("end terrace")
        || raw.contains("end of terrace")
        || raw.contains("mid terrace")
    {
        "terraced"
    } else if raw.contains("maisonette") || raw.contains("apartment") {
        "flat"
    } else if raw.contains("townhouse") || raw.contains("town house") {
        "terraced"
    } else {
        raw.as_str()
    };

    Some(normalized.to_owned())
}

fn extract_region_name(listing: &Listing, config: &ScoringConfig) -> Option<String> {
    if let Some(region) = listing.region.as_deref() {
        let base = region
            .split(',')
            .next()
            .map(str::trim)
            .unwrap_or(region)
            .to_owned();
        return match_region_name(&base, &config.region_priority).or(Some(base));
    }

    extract_region_from_address(&listing.address, &config.region_priority)
}

fn extract_region_from_address(
    address: &str,
    region_priority: &std::collections::BTreeMap<String, f64>,
) -> Option<String> {
    let parts = address
        .split(',')
        .map(|part| part.trim().to_lowercase())
        .collect::<Vec<_>>();

    for part in parts {
        for region in region_priority.keys() {
            let lower_region = region.to_lowercase();
            if !part.starts_with(&lower_region) {
                continue;
            }
            let next_char = part.chars().nth(lower_region.len());
            if next_char.is_none_or(|ch| !ch.is_ascii_lowercase()) {
                return Some(region.clone());
            }
        }
    }

    None
}

fn match_region_name(
    candidate: &str,
    region_priority: &std::collections::BTreeMap<String, f64>,
) -> Option<String> {
    let normalized = candidate.trim().to_lowercase();
    region_priority
        .keys()
        .find(|key| key.to_lowercase() == normalized)
        .cloned()
}

fn property_type_score(property_type: Option<&str>, config: &ScoringConfig) -> f64 {
    let Some(value) = property_type else {
        return 0.7;
    };

    if let Some(score) = config.liveability.property_type.get(value) {
        return score / 100.0;
    }

    let lower = value.to_lowercase();
    if lower.contains("house") || lower.contains("detach") || lower.contains("cottage") {
        return 0.9;
    }
    if lower.contains("flat") || lower.contains("apartment") || lower.contains("studio") {
        return 0.6;
    }

    0.7
}

fn station_proximity_utility(miles: f64) -> f64 {
    if miles <= 0.5 {
        return 1.0;
    }
    exponential_decay(miles - 0.5, 1.5)
}

fn broadband_utility(percent: f64) -> f64 {
    sigmoid((percent - 50.0) * 0.08)
}

fn imd_decile_to_score(decile: i64) -> f64 {
    let bounded = decile.clamp(1, 10) as f64;
    (bounded - 1.0) / 9.0
}

fn epc_band_to_numeric(band: Option<&EpcBand>) -> Option<f64> {
    match band {
        Some(EpcBand::A) => Some(100.0),
        Some(EpcBand::B) => Some(85.0),
        Some(EpcBand::C) => Some(70.0),
        Some(EpcBand::D) => Some(55.0),
        Some(EpcBand::E) => Some(40.0),
        Some(EpcBand::F) => Some(25.0),
        Some(EpcBand::G) => Some(10.0),
        None => None,
    }
}

fn epc_band_str(band: &EpcBand) -> String {
    match band {
        EpcBand::A => "A",
        EpcBand::B => "B",
        EpcBand::C => "C",
        EpcBand::D => "D",
        EpcBand::E => "E",
        EpcBand::F => "F",
        EpcBand::G => "G",
    }
    .to_owned()
}

fn get_heating_cost(band: Option<&EpcBand>, costs: &HeatingCosts) -> f64 {
    match band {
        Some(EpcBand::A) => costs.a,
        Some(EpcBand::B) => costs.b,
        Some(EpcBand::C) => costs.c,
        Some(EpcBand::D) => costs.d,
        Some(EpcBand::E) => costs.e,
        Some(EpcBand::F) => costs.f,
        Some(EpcBand::G) => costs.g,
        None => costs.d,
    }
}

fn clamp(value: f64, min: f64, max: f64) -> f64 {
    value.max(min).min(max)
}

fn sigmoid(value: f64) -> f64 {
    1.0 / (1.0 + (-value).exp())
}

fn exponential_decay(value: f64, rate: f64) -> f64 {
    (-rate * value.max(0.0)).exp()
}

fn round_to(value: f64, decimals: usize) -> f64 {
    let factor = 10f64.powi(decimals as i32);
    (value * factor).round() / factor
}

#[cfg(test)]
mod tests {
    use crate::config::default_scoring_config;
    use crate::schema::listing::{
        Agent, AreaCodeName, AreaMetrics, CrimeMetrics, ExtractionStatus, FloodRisk, GardenType,
        GeoLocation, ImdMetrics, IncomeMetrics, Lettings, Listing, ListingImage, ListingStatus,
        MapViews, PortalIds, RemoteLocalAsset,
    };

    use super::{calculate_percentile, detect_garden_type, score_listings_with_config};

    #[test]
    fn scores_listing_batch() {
        let app_config = crate::config::AppConfig {
            search: crate::config::SearchConfig {
                use_api: true,
                locations: vec![crate::config::Location {
                    id: "REGION^1208".to_owned(),
                    name: "Shrewsbury".to_owned(),
                }],
                filters: crate::config::SearchFilters {
                    min_bedrooms: 2,
                    max_bedrooms: 3,
                    min_price: 700,
                    max_price: 1300,
                    property_types: vec!["detached".to_owned()],
                    include_let_agreed: false,
                    radius: 0.0,
                    dont_show: vec![],
                    must_have: vec!["garden".to_owned()],
                },
            },
            fetch: crate::config::FetchConfig {
                delay_ms: 3000,
                max_listings: 100,
                max_retries: 3,
                ..crate::config::FetchConfig::default()
            },
            scoring: default_scoring_config(),
        };

        let listings = vec![sample_listing("1", 1200), sample_listing("2", 900)];
        let scored = score_listings_with_config(&listings, &app_config);

        assert_eq!(scored.len(), 2);
        assert!(scored.iter().all(|listing| listing.scores.is_some()));
    }

    #[test]
    fn garden_detection_respects_word_boundaries() {
        let mut listing = sample_listing("garden-boundary", 1000);
        listing.description = "easy to maintain gardens near shops".to_owned();
        listing.notes.clear();
        assert_eq!(detect_garden_type(&listing), GardenType::None);

        listing.description = "bright lounge and private garden".to_owned();
        assert_eq!(detect_garden_type(&listing), GardenType::Private);
    }

    #[test]
    fn percentile_handles_uniform_values_neutrally() {
        let sorted = vec![100.0, 100.0, 100.0];
        assert_eq!(calculate_percentile(100.0, &sorted, false), 50.0);
        assert_eq!(calculate_percentile(100.0, &sorted, true), 50.0);
    }

    #[test]
    fn percentile_gives_duplicate_values_the_same_midpoint_rank() {
        let sorted = vec![900.0, 900.0, 1200.0, 1500.0];
        let percentile = calculate_percentile(900.0, &sorted, false);
        assert!(
            (percentile - 25.0).abs() < 0.001,
            "unexpected percentile: {percentile}"
        );

        let inverted = calculate_percentile(900.0, &sorted, true);
        assert!(
            (inverted - 75.0).abs() < 0.001,
            "unexpected inverted percentile: {inverted}"
        );
    }

    fn sample_listing(id: &str, price: i64) -> Listing {
        Listing {
            id: id.to_owned(),
            portal_ids: PortalIds {
                rightmove: Some(format!("rm-{id}")),
                zoopla: None,
                onthemarket: None,
            },
            uprn: None,
            uprn_source: None,
            uprn_confidence: None,
            url: format!("https://www.rightmove.co.uk/properties/{id}"),
            location: GeoLocation {
                lat: 51.5,
                lng: -0.12,
                pin_type: None,
            },
            postcode: "SW1A 1AA".to_owned(),
            address: "10 Example Street, Shrewsbury".to_owned(),
            region: Some("Shrewsbury".to_owned()),
            google_maps_url: "https://maps.example.com".to_owned(),
            google_maps_street_view_url: "https://maps.example.com/street".to_owned(),
            area: AreaMetrics {
                lsoa: AreaCodeName::default(),
                msoa: AreaCodeName::default(),
                imd: ImdMetrics {
                    rank: Some(1000),
                    decile: Some(8),
                    score: Some(20.0),
                },
                income: IncomeMetrics {
                    bhc: Some(45000.0),
                    ahc: Some(38000.0),
                },
                social_housing_pct: Some(10.0),
                population: Some(12000),
                flood_risk: FloodRisk {
                    level: Some("low".to_owned()),
                    source: Some("ea".to_owned()),
                },
                crime: CrimeMetrics {
                    count_12m: Some(100),
                    rate_per_1k: Some(45.0),
                    violent_12m: Some(20),
                    burglary_12m: Some(8),
                    robbery_12m: Some(2),
                    band: None,
                    trend: None,
                    updated_at: Some("2026-01-01T00:00:00.000Z".to_owned()),
                },
            },
            price,
            price_display: format!("£{price} pcm"),
            bedrooms: 2,
            bathrooms: 1,
            property_type: "detached".to_owned(),
            description: "Private garden and gas central heating. pets allowed.".to_owned(),
            notes: vec![],
            images: vec![ListingImage {
                remote: "https://img.example.com/1.jpg".to_owned(),
                local: None,
            }],
            floorplan: RemoteLocalAsset::default(),
            epc: RemoteLocalAsset::default(),
            map_views: MapViews::default(),
            epc_rating: Some(crate::schema::listing::EpcBand::C),
            floor_area_sqm: Some(70.0),
            epc_lodgement_date: None,
            epc_address_match: None,
            epc_search_url: None,
            nearest_stations: vec![crate::schema::listing::StationDistance {
                name: "Station".to_owned(),
                distance: 0.6,
                unit: "miles".to_owned(),
            }],
            gigabit_availability: Some(82.0),
            listed_date: Some("2026-02-01".to_owned()),
            lettings: Lettings {
                available_date: Some("2026-03-01".to_owned()),
                deposit: Some(1500),
            },
            agent: Agent {
                name: Some("Agent".to_owned()),
                phone: Some("0000".to_owned()),
            },
            assessment: None,
            assessed_at: None,
            assessed_score: None,
            scores: None,
            fetched_at: "2026-03-01T00:00:00.000Z".to_owned(),
            extraction_status: ExtractionStatus::Success,
            status: ListingStatus::Active,
            notion_page_id: None,
        }
    }
}
