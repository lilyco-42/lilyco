CREATE TABLE IF NOT EXISTS records (
    id BIGINT NOT NULL AUTO_INCREMENT PRIMARY KEY,
    collection VARCHAR(64) CHARACTER SET ascii COLLATE ascii_bin NOT NULL,
    payload LONGTEXT CHARACTER SET utf8mb4 NOT NULL CHECK (JSON_VALID(payload)),
    INDEX records_by_collection (collection, id)
) ENGINE=InnoDB;
