-- Drill collections share the practice engine with exam papers while keeping
-- catalog, progress and prediction queries scoped by kind.
ALTER TABLE papers ADD COLUMN kind TEXT NOT NULL DEFAULT 'exam';
ALTER TABLE papers ADD COLUMN collection_id TEXT;
ALTER TABLE papers ADD COLUMN collection_title TEXT;
CREATE INDEX IF NOT EXISTS idx_bank_papers_kind_collection
    ON papers(kind, collection_id);
