-- Material JSON now stores only {topic,text}. Legacy section/source URL columns
-- were artificial topic identities, not evidence links. Old groups_json values
-- remain intact and are decoded by serde, preserving history and retry records.
-- Keep the original historical data_analysis_sources/material_seen tables untouched.
ALTER TABLE data_analysis_groups DROP COLUMN source_url;
ALTER TABLE data_analysis_groups DROP COLUMN section;
