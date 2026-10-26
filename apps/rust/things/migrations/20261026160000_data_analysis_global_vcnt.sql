-- Global card-level weighted fair scheduler (vcnt) and answer-only accuracy.
-- Keep legacy fair_count/category tables for rollback; only vcnt is scheduled.
ALTER TABLE data_analysis_card_usage
  ADD COLUMN vcnt INTEGER NOT NULL DEFAULT 0 CHECK(vcnt >= 0);
UPDATE data_analysis_card_usage SET vcnt =
  CASE WHEN fair_count > 9007199254740990 THEN 9007199254740990 * 1024
       ELSE fair_count * 1024 END;

ALTER TABLE data_analysis_results ADD COLUMN card_id TEXT;
ALTER TABLE data_analysis_results
  ADD COLUMN was_answered INTEGER NOT NULL DEFAULT 0 CHECK(was_answered IN (0,1));

-- Rebuild card attribution from the original immutable per-run groups, not
-- from the current flashcards folder. Replays are separate submitted runs.
-- Old NULL answers were historically marked as incorrect; do NOT count them.
UPDATE data_analysis_results
SET card_id = (
      SELECT json_extract(run.groups_json,
        '$[' || CAST((data_analysis_results.question_no-1)/5 AS INTEGER) ||
        '].questions[' || ((data_analysis_results.question_no-1)%5) || '].card_id')
      FROM data_analysis_runs run WHERE run.id=data_analysis_results.run_id
    ),
    was_answered = COALESCE((
      SELECT CASE
        WHEN json_type(run.answers_json,
          '$[' || (data_analysis_results.question_no-1) || ']') = 'integer'
        THEN 1 ELSE 0 END
      FROM data_analysis_runs run WHERE run.id=data_analysis_results.run_id
    ),0);

CREATE INDEX data_analysis_results_card_answered_idx
  ON data_analysis_results(card_id,was_answered);
