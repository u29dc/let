#![forbid(unsafe_code)]

use std::path::Path;

use crate::config::{AppConfig, ScoringConfig};
use crate::errors::Result;
use crate::schema::assessment::AssessmentResult;
use crate::schema::listing::{Listing, ListingsFile};
use crate::schema::search::{SearchDiffResult, SearchDiscoverResult, SearchLocation};

pub trait ConfigService {
    fn load_config(&self, path: Option<&Path>) -> Result<AppConfig>;
    fn load_scoring_config(&self, path: Option<&Path>) -> ScoringConfig;
}

pub trait SearchService {
    fn resolve_location(&self, query: &str) -> Result<Vec<SearchLocation>>;
    fn discover_locations(&self) -> Result<SearchDiscoverResult>;
    fn diff_locations(&self, ids: &[String]) -> Result<SearchDiffResult>;
}

pub trait FetchService {
    fn fetch(&self, ids: &[String], skip_images: bool, skip_epc: bool) -> Result<Vec<Listing>>;
}

pub trait ViewService {
    fn list(
        &self,
        region: Option<&str>,
        min_score: Option<f64>,
        top: Option<usize>,
    ) -> Result<Vec<Listing>>;
    fn detail(&self, id: &str) -> Result<Listing>;
}

pub trait ScoreService {
    fn compute_all(&self) -> Result<Vec<Listing>>;
    fn explain(&self, id: &str) -> Result<Listing>;
}

pub trait AssessService {
    fn candidates(&self, top: Option<usize>) -> Result<Vec<Listing>>;
    fn context(&self, id: &str) -> Result<Listing>;
    fn submit(&self, id: &str, assessment_json: &str) -> Result<AssessmentResult>;
}

pub trait ExportService {
    fn export_json(&self, output: Option<&Path>) -> Result<ListingsFile>;
    fn export_notion(&self, dry_run: bool) -> Result<usize>;
}

pub trait OpsService {
    fn patch(&self, id: &str, patch_json: &str, dry_run: bool) -> Result<Listing>;
    fn verify(&self, dry_run: bool) -> Result<Vec<Listing>>;
    fn prune(&self, dry_run: bool, region: Option<&str>, min_score: Option<f64>) -> Result<usize>;
}

pub trait SourceBuildService {
    fn list_sources(&self) -> Vec<&'static str>;
    fn build_one(&self, source: &str) -> Result<()>;
    fn build_all(&self, jobs: usize) -> Result<()>;
}
