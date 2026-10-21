//! User-facing DTOs. Secrets and correct indices never enter an active run.
use serde::{Deserialize, Serialize};
use crate::domain::data_analysis::Material;
pub(crate) const ANALYSIS_ROOT:&str="/api/v1/study/data-analysis";
#[derive(Clone, Debug, Deserialize)]
pub(crate) struct AnalysisStatus {
    pub configured:bool, pub model:Option<String>, pub cached_sources:i64,
    pub verified_groups:i64, pub weak_skills:Vec<String>,
}
#[derive(Debug, Deserialize)]
pub(crate) struct PrepareTask {pub job_id:String}
#[derive(Clone, Debug, Deserialize)]
pub(crate) struct PrepareProgress { pub state:String, pub verified:usize, pub message:String, pub run_id:Option<String>, }
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub(crate) struct ViewQuestion {pub question:String, pub options:[String;4], pub skill:String}
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub(crate) struct ViewGroup {pub material:Material, pub questions:Vec<ViewQuestion>}
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub(crate) struct AnalysisRun { pub id:String, pub status:String, pub started_at_ms:Option<i64>,
    pub deadline_at_ms:Option<i64>, pub duration_seconds:u64,
    pub groups:Vec<ViewGroup>, pub answers:Vec<Option<usize>>,
}
#[derive(Clone, Debug, Deserialize)]
pub(crate) struct GradedQuestion {pub number:usize, pub chosen:Option<usize>,
    pub correct_index:usize, pub answer:String, pub explanation:String,
    pub topic:String,pub skill:String,pub evidence:String,
}
#[derive(Clone, Debug, Deserialize)]
pub(crate) struct AnalysisResult {pub correct:usize,pub total:usize,pub questions:Vec<GradedQuestion>}
#[derive(Serialize)]
pub(crate) struct SaveAnswer {pub choice:usize}
