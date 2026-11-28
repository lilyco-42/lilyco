//! Offline transfer of Rapid records between stores. A transfer preserves
//! record IDs and the allocation watermark. Import commits only to an empty store.

use std::{
    fs::File,
    io::{BufReader, BufWriter, Write},
    path::Path,
};

use serde::{Deserialize, Serialize};

use crate::store::StoreError;

const FORMAT: u16 = 1;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Snapshot {
    pub format: u16,
    pub watermark: u32,
    pub records: Vec<Row>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct Row {
    pub id: u32,
    pub collection: String,
    pub payload: String,
}

impl Snapshot {
    pub fn new(watermark: u32, records: Vec<Row>) -> Self {
        Self {
            format: FORMAT,
            watermark,
            records,
        }
    }

    pub fn validate(&self) -> Result<(), StoreError> {
        if self.format != FORMAT {
            return Err(StoreError::Snapshot("unsupported format version"));
        }
        let mut previous = 0;
        for row in &self.records {
            if row.id <= previous || row.id > self.watermark {
                return Err(StoreError::Snapshot(
                    "IDs must be positive, sorted, unique and below the watermark",
                ));
            }
            let name = &row.collection;
            if name.len() > 64
                || !name.starts_with(|ch: char| ch.is_ascii_lowercase())
                || !name
                    .bytes()
                    .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == b'-')
            {
                return Err(StoreError::Snapshot("invalid portable collection name"));
            }
            serde_json::from_str::<&serde_json::value::RawValue>(&row.payload)?;
            previous = row.id;
        }
        Ok(())
    }
}

pub async fn export(storage: &crate::config::Storage, path: &Path) -> Result<usize, StoreError> {
    let snapshot = crate::store::Store::connect(storage)
        .await?
        .snapshot()
        .await?;
    snapshot.validate()?;
    let count = snapshot.records.len();
    let parent = path
        .parent()
        .filter(|dir| !dir.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    {
        let mut writer = BufWriter::new(file.as_file_mut());
        serde_json::to_writer(&mut writer, &snapshot)?;
        writer.flush()?;
    }
    file.as_file().sync_all()?;
    file.persist_noclobber(path)
        .map_err(|error| StoreError::Io(error.error))?;
    Ok(count)
}

pub async fn import(storage: &crate::config::Storage, path: &Path) -> Result<usize, StoreError> {
    let snapshot: Snapshot = serde_json::from_reader(BufReader::new(File::open(path)?))?;
    snapshot.validate()?;
    let count = snapshot.records.len();
    crate::store::Store::connect(storage)
        .await?
        .restore(snapshot)
        .await?;
    Ok(count)
}
