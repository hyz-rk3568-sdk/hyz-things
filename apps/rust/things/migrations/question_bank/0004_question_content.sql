-- Cache parsed question content so practice never rescans Markdown.
-- Answers and explanations remain server-only and are read through the
-- application review use case.
ALTER TABLE questions ADD COLUMN stem TEXT;
ALTER TABLE questions ADD COLUMN options_markdown TEXT;
ALTER TABLE questions ADD COLUMN topics TEXT;
ALTER TABLE questions ADD COLUMN materials TEXT;
ALTER TABLE questions ADD COLUMN issues TEXT;
ALTER TABLE questions ADD COLUMN gradable INTEGER NOT NULL DEFAULT 0;
ALTER TABLE questions ADD COLUMN correct_answer TEXT;
ALTER TABLE questions ADD COLUMN explanation TEXT;

CREATE TABLE IF NOT EXISTS paper_materials (
    paper_id TEXT NOT NULL REFERENCES papers(id),
    material_id TEXT NOT NULL,
    ordinal INTEGER NOT NULL,
    name TEXT NOT NULL,
    markdown TEXT NOT NULL,
    PRIMARY KEY (paper_id, material_id)
);
CREATE INDEX IF NOT EXISTS idx_bank_paper_materials_order
    ON paper_materials(paper_id, ordinal);
