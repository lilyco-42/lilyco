DEFINE TABLE IF NOT EXISTS rapid_records SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS number ON rapid_records TYPE int ASSERT $value > 0 AND $value <= 4294967295;
DEFINE FIELD IF NOT EXISTS collection ON rapid_records TYPE string;
DEFINE FIELD IF NOT EXISTS payload ON rapid_records TYPE string;
DEFINE INDEX IF NOT EXISTS rapid_by_collection ON rapid_records FIELDS collection, number;
DEFINE TABLE IF NOT EXISTS rapid_counters SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS watermark ON rapid_counters TYPE int;
DEFINE FIELD IF NOT EXISTS revision ON rapid_counters TYPE int;
INSERT IGNORE INTO rapid_counters (id, watermark, revision) VALUES (rapid_counters:global, 0, 0);
