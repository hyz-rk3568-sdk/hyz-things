//! AI-generated question features: data analysis and independent drills.
//! Logic judgment and quantitative relations share one drill implementation.
use super::*;

mod activity;
mod comprehensive;
mod logic;
mod prompt;

pub(crate) use activity::*;
pub(crate) use comprehensive::*;
pub(crate) use logic::*;

use prompt::*;
