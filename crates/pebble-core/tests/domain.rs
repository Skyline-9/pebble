#![forbid(unsafe_code)]

//! Domain contract integration tests.

use std::path::Path;

use pebble_core::domain::{
    ChunkId, Citation, EvidenceDiagnostic, EvidenceItem, EvidencePacket, FileId, GenerationId,
    RepositoryId, ScoreExplanation, SymbolId, WorktreeRevision,
};
use pebble_core::repository::StateLayout;

fn expected_id(kind: &str, parts: &[&str]) -> String {
    let mut hasher = blake3::Hasher::new();
    for part in std::iter::once(&kind).chain(parts) {
        let bytes = part.as_bytes();
        let length = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
        hasher.update(&length.to_le_bytes());
        hasher.update(bytes);
    }
    format!("{kind}_{}", hasher.finalize().to_hex())
}

#[test]
fn external_ids_accept_only_portable_characters() {
    for valid in ["repo", "repo.main_01-Z", "01K0ABCDEF"] {
        assert!(RepositoryId::try_from(valid.to_owned()).is_ok(), "{valid}");
        assert!(GenerationId::try_from(valid.to_owned()).is_ok(), "{valid}");
    }

    for invalid in ["", "repo/name", "repo name", "repo:name", "répo"] {
        assert!(
            RepositoryId::try_from(invalid.to_owned()).is_err(),
            "{invalid}"
        );
        assert!(
            GenerationId::try_from(invalid.to_owned()).is_err(),
            "{invalid}"
        );
        assert!(FileId::try_from(invalid.to_owned()).is_err(), "{invalid}");
        assert!(ChunkId::try_from(invalid.to_owned()).is_err(), "{invalid}");
        assert!(SymbolId::try_from(invalid.to_owned()).is_err(), "{invalid}");
    }
}

#[test]
fn identifiers_round_trip_through_serde_with_validation() -> Result<(), Box<dyn std::error::Error>>
{
    let repository = RepositoryId::try_from("repo.main".to_owned())?;
    let encoded = serde_json::to_string(&repository)?;
    let decoded: RepositoryId = serde_json::from_str(&encoded)?;

    assert_eq!(decoded, repository);
    assert!(serde_json::from_str::<RepositoryId>("\"repo/name\"").is_err());
    Ok(())
}

#[test]
fn content_ids_use_unambiguous_blake3_inputs() -> Result<(), Box<dyn std::error::Error>> {
    let repository = RepositoryId::try_from("acme-pebble".to_owned())?;

    let file = FileId::derive(&repository, "src/lib.rs");
    let chunk = ChunkId::derive(&file, 10, 24, 2, "content digest");
    let symbol = SymbolId::derive(&repository, "rust", "pebble::run");

    assert_eq!(
        file.to_string(),
        expected_id("file", &["acme-pebble", "src/lib.rs"])
    );
    assert_eq!(
        chunk.to_string(),
        expected_id("chunk", &[file.as_str(), "10", "24", "2", "content digest"],)
    );
    assert_eq!(
        symbol.to_string(),
        expected_id("symbol", &["acme-pebble", "rust", "pebble::run"])
    );
    assert_ne!(
        FileId::derive(&RepositoryId::try_from("ab".to_owned())?, "c").to_string(),
        FileId::derive(&RepositoryId::try_from("a".to_owned())?, "bc").to_string()
    );
    Ok(())
}

#[test]
fn revisions_have_stable_clean_and_dirty_formats() -> Result<(), Box<dyn std::error::Error>> {
    let clean = WorktreeRevision::clean("0123456789abcdef")?;
    let dirty = WorktreeRevision::dirty("0123456789abcdef", "fedcba9876543210")?;

    assert_eq!(clean.to_string(), "0123456789abcdef");
    assert_eq!(dirty.to_string(), "0123456789abcdef+dirty.fedcba9876543210");
    assert_eq!(dirty.base_oid(), "0123456789abcdef");
    assert_eq!(dirty.dirty_digest(), Some("fedcba9876543210"));
    assert!(WorktreeRevision::clean("").is_err());
    assert!(WorktreeRevision::clean("ABCDEF").is_err());
    assert!(WorktreeRevision::clean("not-hex").is_err());
    assert!(WorktreeRevision::dirty("0123456789abcdef", "").is_err());
    assert!(WorktreeRevision::dirty("0123456789abcdef", "ABCDEF").is_err());
    assert!(WorktreeRevision::dirty("0123456789abcdef", "not-hex").is_err());
    Ok(())
}

#[test]
fn citations_require_resolvable_normalized_ranges() -> Result<(), Box<dyn std::error::Error>> {
    let repository = RepositoryId::try_from("acme-pebble".to_owned())?;
    let revision = WorktreeRevision::clean("0123456789abcdef")?;
    let citation = Citation::new(repository.clone(), revision.clone(), "src/lib.rs", 1, 8)?;

    assert_eq!(citation.repository(), &repository);
    assert_eq!(citation.revision(), &revision);
    assert_eq!(citation.path(), "src/lib.rs");
    assert_eq!(citation.start_line(), 1);
    assert_eq!(citation.end_line(), 8);
    for path in [
        "",
        "/src/lib.rs",
        "src\\lib.rs",
        "./src/lib.rs",
        "src/../lib.rs",
    ] {
        assert!(
            Citation::new(repository.clone(), revision.clone(), path, 1, 8).is_err(),
            "{path}"
        );
    }
    assert!(Citation::new(repository.clone(), revision.clone(), "src/lib.rs", 0, 8).is_err());
    assert!(Citation::new(repository, revision, "src/lib.rs", 8, 7).is_err());
    Ok(())
}

#[test]
fn evidence_budget_is_bounded_inclusively() {
    assert!(EvidencePacket::new(1_000, Vec::new(), Vec::new()).is_ok());
    assert!(EvidencePacket::new(6_000, Vec::new(), Vec::new()).is_ok());
    assert!(EvidencePacket::new(32_000, Vec::new(), Vec::new()).is_ok());
    assert!(EvidencePacket::new(999, Vec::new(), Vec::new()).is_err());
    assert!(EvidencePacket::new(32_001, Vec::new(), Vec::new()).is_err());
    assert!(
        serde_json::from_str::<EvidencePacket>(
            r#"{"budget_tokens":999,"items":[],"diagnostics":[]}"#
        )
        .is_err()
    );
}

#[test]
fn evidence_packets_carry_explanations_and_diagnostics() -> Result<(), Box<dyn std::error::Error>> {
    let repository = RepositoryId::try_from("acme-pebble".to_owned())?;
    let revision = WorktreeRevision::clean("0123456789abcdef")?;
    let citation = Citation::new(repository, revision, "src/lib.rs", 1, 8)?;
    let item = EvidenceItem {
        citation,
        content: "pub fn run() {}".to_owned(),
        score_explanations: vec![ScoreExplanation {
            scorer: "exact_symbol".to_owned(),
            score: 1.0,
            explanation: "exact symbol match".to_owned(),
        }],
    };
    let diagnostic = EvidenceDiagnostic {
        code: "truncated".to_owned(),
        message: "one candidate omitted by the budget".to_owned(),
    };
    let packet = EvidencePacket::new(1_000, vec![item], vec![diagnostic])?;

    assert_eq!(packet.budget_tokens(), 1_000);
    assert_eq!(
        packet.items()[0].score_explanations[0].scorer,
        "exact_symbol"
    );
    assert_eq!(packet.diagnostics()[0].code, "truncated");
    Ok(())
}

#[test]
fn state_layout_is_versioned_and_repository_scoped() -> Result<(), Box<dyn std::error::Error>> {
    let repository = RepositoryId::try_from("acme-pebble".to_owned())?;
    let layout = StateLayout::new(Path::new("/home/alice"));

    assert_eq!(layout.root(), Path::new("/home/alice/.pebble/v1"));
    assert_eq!(
        layout.generations(&repository),
        Path::new("/home/alice/.pebble/v1/repos/acme-pebble/generations")
    );
    Ok(())
}
