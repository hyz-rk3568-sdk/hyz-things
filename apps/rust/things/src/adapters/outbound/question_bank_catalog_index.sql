-- Keep module counts and candidate selection on small index pages.
-- The TEXT question ID is explicit because secondary indexes otherwise carry rowid.
CREATE INDEX IF NOT EXISTS idx_bank_questions_catalog_cover
    ON questions(paper_id, module, category, number, id);
