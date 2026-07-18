#![forbid(unsafe_code)]

//! Bounded SCIP import integration tests.

use pebble_core::domain::RepositoryId;
use pebble_core::ingestion::{EdgeKind, ScipError, ScipImporter};

const MIB: usize = 1_024 * 1_024;

#[test]
fn imports_symbols_edges_and_unresolved_external_symbols() -> Result<(), Box<dyn std::error::Error>>
{
    let extraction = importer()?.import(&valid_index())?;

    assert_eq!(extraction.symbols().len(), 1);
    let symbol = &extraction.symbols()[0];
    assert_eq!(symbol.name(), "answer");
    assert_eq!((symbol.start_line(), symbol.end_line()), (1, 1));

    assert_eq!(extraction.edges().len(), 3);
    assert_eq!(extraction.edges()[0].kind(), EdgeKind::Defines);
    assert_eq!(extraction.edges()[0].target(), symbol.id().as_str());
    assert_eq!(extraction.edges()[1].kind(), EdgeKind::References);
    assert_eq!(extraction.edges()[1].target(), symbol.id().as_str());
    assert_eq!(extraction.edges()[2].kind(), EdgeKind::References);
    assert_eq!(
        extraction.edges()[2].target(),
        "scip rust pkg 1.0 dep/Thing#"
    );
    assert_eq!(
        extraction.unresolved_external_symbols(),
        &["scip rust pkg 1.0 dep/Thing#"]
    );
    Ok(())
}

#[test]
fn skips_unknown_fields_of_every_wire_type() -> Result<(), Box<dyn std::error::Error>> {
    let mut bytes = valid_index();
    field_varint(100, 7, &mut bytes);
    field_fixed64(101, 7, &mut bytes);
    field_bytes(102, b"ignored", &mut bytes);
    field_group(103, &[], &mut bytes);
    field_fixed32(104, 7, &mut bytes);

    let extraction = importer()?.import(&bytes)?;
    assert_eq!(extraction.symbols().len(), 1);
    Ok(())
}

#[test]
fn rejects_truncated_varints_and_impossible_wire_values() -> Result<(), Box<dyn std::error::Error>>
{
    assert!(importer()?.import(&[0x80]).is_err());
    let mut impossible_length = vec![0x12];
    impossible_length.extend_from_slice(&[0xff; 10]);
    assert!(importer()?.import(&impossible_length).is_err());
    assert!(importer()?.import(&[0x0f]).is_err());
    assert!(importer()?.import(&[0x0c]).is_err());
    let mut impossible_field = Vec::new();
    varint((u64::from(0x2000_0000_u32)) << 3, &mut impossible_field);
    impossible_field.push(0);
    assert!(importer()?.import(&impossible_field).is_err());
    Ok(())
}

#[test]
fn rejects_input_above_sixty_four_mib() -> Result<(), Box<dyn std::error::Error>> {
    assert!(importer()?.import(&vec![0; 64 * MIB + 1]).is_err());
    Ok(())
}

#[test]
fn rejects_messages_nested_beyond_depth_sixty_four() -> Result<(), Box<dyn std::error::Error>> {
    let mut document = field(1, b"src/lib.rs");
    for _ in 0..33 {
        document = field(7, &document);
        document = field(3, &document);
    }
    assert!(importer()?.import(&field(2, &document)).is_err());

    let mut groups = Vec::new();
    for _ in 0..65 {
        key(100, 3, &mut groups);
    }
    for _ in 0..65 {
        key(100, 4, &mut groups);
    }
    assert!(importer()?.import(&groups).is_err());
    Ok(())
}

#[test]
fn resolves_symbols_defined_in_another_document() -> Result<(), Box<dyn std::error::Error>> {
    let global = "scip rust pkg 1.0 crate/answer.";
    let definition = document(
        &[occurrence(&[0, 0, 6], global, 1)],
        &[symbol(global, "answer")],
    );
    let mut reference = document(&[occurrence(&[0, 0, 6], global, 0)], &[]);
    replace_path(&mut reference, b"src/use.rs");
    let mut index = field(2, &definition);
    field_bytes(2, &reference, &mut index);

    let extraction = importer()?.import(&index)?;
    assert_eq!(extraction.symbols().len(), 1);
    assert!(extraction.unresolved_external_symbols().is_empty());
    assert_eq!(
        extraction.edges()[1].target(),
        extraction.symbols()[0].id().as_str()
    );
    Ok(())
}

#[test]
fn rejects_more_than_one_hundred_thousand_documents() -> Result<(), Box<dyn std::error::Error>> {
    let mut index = Vec::new();
    for _ in 0..=100_000 {
        field_bytes(2, &[], &mut index);
    }
    assert!(importer()?.import(&index).is_err());
    Ok(())
}

#[test]
fn rejects_more_than_one_million_symbols() -> Result<(), Box<dyn std::error::Error>> {
    let mut document = field(1, b"src/lib.rs");
    for _ in 0..=1_000_000 {
        field_bytes(3, &[], &mut document);
    }
    assert!(importer()?.import(&field(2, &document)).is_err());
    Ok(())
}

#[test]
fn rejects_more_than_one_million_occurrences_per_document() -> Result<(), Box<dyn std::error::Error>>
{
    let occurrence = occurrence(&[0, 0, 0], "", 0);
    let mut document = field(1, b"src/lib.rs");
    for _ in 0..=1_000_000 {
        field_bytes(2, &occurrence, &mut document);
    }
    assert!(importer()?.import(&field(2, &document)).is_err());
    Ok(())
}

#[test]
fn rejects_strings_above_one_mib() -> Result<(), Box<dyn std::error::Error>> {
    let document = field(1, &vec![b'a'; MIB + 1]);
    assert!(importer()?.import(&field(2, &document)).is_err());
    Ok(())
}

#[test]
fn converts_half_open_ranges_to_inclusive_one_based_lines() -> Result<(), Box<dyn std::error::Error>>
{
    assert_symbol_range(&[0, 0, 1, 0], (1, 1))?;
    assert_symbol_range(&[2, 3, 7], (3, 3))?;
    assert_symbol_range(&[2, 3, 4, 1], (3, 5))?;
    Ok(())
}

#[test]
fn cites_empty_ranges_on_their_containing_start_line() -> Result<(), Box<dyn std::error::Error>> {
    assert_symbol_range(&[0, 0, 0], (1, 1))?;
    assert_symbol_range(&[2, 3, 3], (3, 3))?;
    assert_symbol_range(&[0, 0, 0, 0], (1, 1))?;
    assert_symbol_range(&[2, 7, 2, 7], (3, 3))?;
    Ok(())
}

#[test]
fn rejects_invalid_and_out_of_order_ranges() -> Result<(), Box<dyn std::error::Error>> {
    for range in [
        &[][..],
        &[0, 1][..],
        &[0, 4, 3][..],
        &[0, 1, 0, 0][..],
        &[2, 0, 1, 0][..],
    ] {
        let occurrence = occurrence(range, "local 0", 0);
        let document = document(&[occurrence], &[]);
        assert!(importer()?.import(&field(2, &document)).is_err());
    }
    Ok(())
}

#[test]
fn rejects_packed_range_on_fifth_value_without_scanning_remainder()
-> Result<(), Box<dyn std::error::Error>> {
    let mut packed = vec![0; MIB];
    packed.push(0x80);
    let occurrence = field(1, &packed);
    let document = document(&[occurrence], &[]);

    assert_eq!(
        importer()?.import(&field(2, &document)),
        Err(ScipError::InvalidRange)
    );
    Ok(())
}

fn assert_symbol_range(
    range: &[i32],
    expected: (u32, u32),
) -> Result<(), Box<dyn std::error::Error>> {
    let local = "local 0";
    let definition = occurrence(range, local, 1);
    let information = symbol(local, "boundary");
    let extraction = importer()?.import(&field(2, &document(&[definition], &[information])))?;
    assert_eq!(extraction.symbols().len(), 1);
    let actual = (
        extraction.symbols()[0].start_line(),
        extraction.symbols()[0].end_line(),
    );
    assert_eq!(actual, expected);
    Ok(())
}

fn importer() -> Result<ScipImporter, Box<dyn std::error::Error>> {
    Ok(ScipImporter::new(RepositoryId::try_from(
        "example.repository".to_owned(),
    )?))
}

fn valid_index() -> Vec<u8> {
    let local = "local 0";
    let external = "scip rust pkg 1.0 dep/Thing#";
    let occurrences = [
        occurrence(&[0, 0, 6], local, 1),
        occurrence(&[1, 0, 6], local, 0),
        occurrence(&[2, 0, 5], external, 0),
    ];
    let local_information = symbol(local, "answer");
    let mut index = field(2, &document(&occurrences, &[local_information]));
    field_bytes(3, &symbol(external, "Thing"), &mut index);
    index
}

fn document(occurrences: &[Vec<u8>], symbols: &[Vec<u8>]) -> Vec<u8> {
    let mut output = field(1, b"src/lib.rs");
    field_bytes(4, b"rust", &mut output);
    for occurrence in occurrences {
        field_bytes(2, occurrence, &mut output);
    }
    for symbol in symbols {
        field_bytes(3, symbol, &mut output);
    }
    output
}

fn replace_path(document: &mut Vec<u8>, path: &[u8]) {
    *document = {
        let mut output = field(1, path);
        output.extend_from_slice(&document[field(1, b"src/lib.rs").len()..]);
        output
    };
}

fn occurrence(range: &[i32], symbol: &str, roles: i32) -> Vec<u8> {
    let mut packed = Vec::new();
    for value in range {
        varint(
            u64::from_ne_bytes(i64::from(*value).to_ne_bytes()),
            &mut packed,
        );
    }
    let mut output = field(1, &packed);
    field_bytes(2, symbol.as_bytes(), &mut output);
    if roles != 0 {
        field_varint(3, u64::try_from(roles).unwrap_or_default(), &mut output);
    }
    output
}

fn symbol(symbol: &str, display_name: &str) -> Vec<u8> {
    let mut output = field(1, symbol.as_bytes());
    field_bytes(6, display_name.as_bytes(), &mut output);
    output
}

fn field(number: u32, contents: &[u8]) -> Vec<u8> {
    let mut output = Vec::new();
    field_bytes(number, contents, &mut output);
    output
}

fn field_bytes(number: u32, contents: &[u8], output: &mut Vec<u8>) {
    key(number, 2, output);
    varint(u64::try_from(contents.len()).unwrap_or(u64::MAX), output);
    output.extend_from_slice(contents);
}

fn field_varint(number: u32, value: u64, output: &mut Vec<u8>) {
    key(number, 0, output);
    varint(value, output);
}

fn field_fixed64(number: u32, value: u64, output: &mut Vec<u8>) {
    key(number, 1, output);
    output.extend_from_slice(&value.to_le_bytes());
}

fn field_group(number: u32, contents: &[u8], output: &mut Vec<u8>) {
    key(number, 3, output);
    output.extend_from_slice(contents);
    key(number, 4, output);
}

fn field_fixed32(number: u32, value: u32, output: &mut Vec<u8>) {
    key(number, 5, output);
    output.extend_from_slice(&value.to_le_bytes());
}

fn key(number: u32, wire: u8, output: &mut Vec<u8>) {
    varint((u64::from(number) << 3) | u64::from(wire), output);
}

fn varint(mut value: u64, output: &mut Vec<u8>) {
    while value >= 0x80 {
        output.push(value.to_le_bytes()[0] | 0x80);
        value >>= 7;
    }
    output.push(value.to_le_bytes()[0]);
}
