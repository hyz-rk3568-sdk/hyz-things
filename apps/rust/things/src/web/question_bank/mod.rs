//! Question Bank front-end feature: 行测 practice and 申论 share one module.
//! 申论 is a subject inside the question bank, not a sibling product module.
use super::*;

mod ai_composer;
mod ai_reply;
mod ai_review;
mod ai_review_api;
mod ai_review_recovery;
mod ai_review_stream;
mod ai_review_timeline;
mod collection_review;
mod manage;
mod ocr_panel;
mod practice;
mod setup;
mod shenlun;
mod source_answer_panel;

pub(crate) use ai_review::*;
pub(crate) use collection_review::*;
pub(crate) use manage::*;
pub(crate) use practice::*;
pub(crate) use shenlun::*;

use ai_composer::*;
use ai_reply::*;
use ai_review_api::*;
use ai_review_recovery::*;
use setup::*;
