#![forbid(unsafe_code)]

//! Sealed flat-vector generation format and brute-force search unit tests.

use std::fs::{self, OpenOptions};
use std::io::{Seek, SeekFrom, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use pebble_core::vectors::flat_index::FlatVectorIndex;
use pebble_core::vectors::format::{VectorFileReader, VectorFileWriter};

const FINGERPRINT_A: [u8; 32] = [7_u8; 32];
const FINGERPRINT_B: [u8; 32] = [9_u8; 32];
const HEADER_ROW_COUNT_OFFSET: u64 = 8 + 4 + 32 + 4;

#[test]
fn writer_reader_round_trips_rows_in_order() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new("round-trip")?;
    let mut writer = VectorFileWriter::create(&fixture.vector, &fixture.ids, 2, FINGERPRINT_A)?;
    writer.write_row("entity_one", &[1.0, 0.0])?;
    writer.write_row("entity_two", &[0.0, 1.0])?;
    writer.finish()?;

    let reader = VectorFileReader::open(&fixture.vector, &fixture.ids, FINGERPRINT_A)?;
    assert_eq!(reader.dimension(), 2);
    assert_eq!(reader.row_count(), 2);
    assert_eq!(reader.entity_id(0), Some("entity_one"));
    assert_eq!(reader.entity_id(1), Some("entity_two"));
    assert_eq!(reader.read_row(0)?, vec![1.0, 0.0]);
    assert_eq!(reader.read_row(1)?, vec![0.0, 1.0]);
    assert!(reader.read_row(2).is_err());
    Ok(())
}

#[test]
fn rejects_a_mismatched_model_fingerprint() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new("fingerprint")?;
    let mut writer = VectorFileWriter::create(&fixture.vector, &fixture.ids, 2, FINGERPRINT_A)?;
    writer.write_row("entity_one", &[1.0, 0.0])?;
    writer.finish()?;

    assert!(VectorFileReader::open(&fixture.vector, &fixture.ids, FINGERPRINT_B).is_err());
    assert!(FlatVectorIndex::open(&fixture.vector, &fixture.ids, FINGERPRINT_B).is_err());
    assert!(VectorFileReader::open(&fixture.vector, &fixture.ids, FINGERPRINT_A).is_ok());
    Ok(())
}

#[test]
fn rejects_a_truncated_vector_file() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new("truncated")?;
    let mut writer = VectorFileWriter::create(&fixture.vector, &fixture.ids, 4, FINGERPRINT_A)?;
    writer.write_row("entity_one", &[1.0, 2.0, 3.0, 4.0])?;
    writer.write_row("entity_two", &[5.0, 6.0, 7.0, 8.0])?;
    writer.finish()?;

    let original_length = fs::metadata(&fixture.vector)?.len();
    let file = OpenOptions::new().write(true).open(&fixture.vector)?;
    file.set_len(original_length.saturating_sub(4))?;

    assert!(VectorFileReader::open(&fixture.vector, &fixture.ids, FINGERPRINT_A).is_err());
    Ok(())
}

#[test]
fn rejects_a_row_count_inconsistent_with_its_file_size() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new("row-count")?;
    let mut writer = VectorFileWriter::create(&fixture.vector, &fixture.ids, 2, FINGERPRINT_A)?;
    writer.write_row("entity_one", &[1.0, 0.0])?;
    writer.finish()?;

    // Overwrite the header's row-count field to claim two rows while only one
    // row of bytes is actually present, and append a second matching line to
    // the entity-ID sidecar so only the fixed-width payload is inconsistent.
    let mut file = OpenOptions::new().write(true).open(&fixture.vector)?;
    file.seek(SeekFrom::Start(HEADER_ROW_COUNT_OFFSET))?;
    file.write_all(&2_u64.to_le_bytes())?;
    let mut ids_file = OpenOptions::new().append(true).open(&fixture.ids)?;
    ids_file.write_all(b"entity_two\n")?;

    assert!(VectorFileReader::open(&fixture.vector, &fixture.ids, FINGERPRINT_A).is_err());
    Ok(())
}

#[test]
fn rejects_an_embedding_length_that_does_not_match_the_dimension()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new("dimension-mismatch")?;
    let mut writer = VectorFileWriter::create(&fixture.vector, &fixture.ids, 3, FINGERPRINT_A)?;
    assert!(writer.write_row("entity_one", &[1.0, 2.0]).is_err());
    Ok(())
}

#[test]
fn rejects_an_invalid_entity_id() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new("invalid-entity")?;
    let mut writer = VectorFileWriter::create(&fixture.vector, &fixture.ids, 1, FINGERPRINT_A)?;
    assert!(writer.write_row("has a space", &[1.0]).is_err());
    assert!(writer.write_row("", &[1.0]).is_err());
    Ok(())
}

#[test]
fn top_k_is_deterministic_and_breaks_ties_by_entity_id() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new("top-k")?;
    let mut writer = VectorFileWriter::create(&fixture.vector, &fixture.ids, 2, FINGERPRINT_A)?;
    writer.write_row("entity_axis_x", &[1.0, 0.0])?;
    writer.write_row("entity_axis_y", &[0.0, 1.0])?;
    writer.write_row("entity_diagonal", &[1.0, 1.0])?;
    writer.write_row("entity_opposite_x", &[-1.0, 0.0])?;
    writer.finish()?;

    let index = FlatVectorIndex::open(&fixture.vector, &fixture.ids, FINGERPRINT_A)?;
    assert_eq!(index.dimension(), 2);
    assert_eq!(index.row_count(), 4);

    let top = index.top_k(&[1.0, 0.0], 4)?;
    let ids = top.iter().map(|(id, _)| id.as_str()).collect::<Vec<_>>();
    assert_eq!(
        ids,
        vec![
            "entity_axis_x",
            "entity_diagonal",
            "entity_axis_y",
            "entity_opposite_x",
        ]
    );
    assert!((top[0].1 - 1.0).abs() < 1e-6);
    assert!((top[3].1 - (-1.0)).abs() < 1e-6);

    let bounded = index.top_k(&[1.0, 0.0], 2)?;
    assert_eq!(bounded.len(), 2);
    assert_eq!(bounded[0].0, "entity_axis_x");
    assert_eq!(bounded[1].0, "entity_diagonal");

    assert!(index.top_k(&[1.0, 0.0], 0).is_err());
    assert!(index.top_k(&[1.0], 1).is_err());
    Ok(())
}

#[test]
fn top_k_produces_identical_results_across_repeated_queries()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new("repeatable")?;
    let mut writer = VectorFileWriter::create(&fixture.vector, &fixture.ids, 3, FINGERPRINT_A)?;
    writer.write_row("entity_first", &[0.2, 0.4, 0.4])?;
    writer.write_row("entity_second", &[0.9, 0.1, 0.0])?;
    writer.write_row("entity_third", &[0.0, 0.0, 1.0])?;
    writer.finish()?;

    let index = FlatVectorIndex::open(&fixture.vector, &fixture.ids, FINGERPRINT_A)?;
    let query = [0.1, 0.2, 0.3];
    let first = index.top_k(&query, 3)?;
    let second = index.top_k(&query, 3)?;
    assert_eq!(first, second);
    Ok(())
}

struct Fixture {
    root: PathBuf,
    vector: PathBuf,
    ids: PathBuf,
}

impl Fixture {
    fn new(label: &str) -> Result<Self, Box<dyn std::error::Error>> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let sequence = NEXT.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "pebble-vectors-{label}-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir_all(&root)?;
        Ok(Self {
            vector: root.join("generation.vec"),
            ids: root.join("generation.ids"),
            root,
        })
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
