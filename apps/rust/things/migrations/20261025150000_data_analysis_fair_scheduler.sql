-- Independent category and per-card fair scheduling counters.
-- selected_count is the actual number of newly prepared original paper questions.
-- fair_count is a virtual counter used only for ordering. New categories/cards
-- inherit the minimum fair_count of their peer pool when first registered.
CREATE TABLE data_analysis_category_usage (
    category TEXT PRIMARY KEY NOT NULL,
    selected_count INTEGER NOT NULL DEFAULT 0 CHECK (selected_count >= 0),
    fair_count INTEGER NOT NULL DEFAULT 0 CHECK (fair_count >= 0)
);

CREATE TABLE data_analysis_card_usage (
    card_id TEXT PRIMARY KEY NOT NULL,
    category TEXT NOT NULL,
    selected_count INTEGER NOT NULL DEFAULT 0 CHECK (selected_count >= 0),
    fair_count INTEGER NOT NULL DEFAULT 0 CHECK (fair_count >= 0)
);
CREATE INDEX data_analysis_card_usage_category_idx
    ON data_analysis_card_usage(category, fair_count);

-- Each counted card must belong to a real completed original paper. The ledger
-- lives in the SAME transaction as run insertion and counter increments.
-- Historical replay does not enter this ledger, even if its groups contain IDs.
CREATE TABLE data_analysis_card_selection_log (
    run_id TEXT NOT NULL,
    card_id TEXT NOT NULL,
    category TEXT NOT NULL,
    selected_at_ms INTEGER NOT NULL,
    PRIMARY KEY (run_id, card_id),
    FOREIGN KEY (run_id) REFERENCES data_analysis_runs(id) ON DELETE CASCADE
);
CREATE INDEX data_analysis_card_selection_log_card_idx
    ON data_analysis_card_selection_log(card_id);
