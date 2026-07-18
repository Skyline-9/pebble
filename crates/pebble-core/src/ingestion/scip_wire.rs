//! Minimal bounded protobuf wire decoding for consumed SCIP fields.

use super::scip::ScipError;

pub(super) const MAX_INPUT_BYTES: usize = 64 * 1_024 * 1_024;
const MAX_DEPTH: usize = 64;
const MAX_DOCUMENTS: usize = 100_000;
const MAX_SYMBOLS: usize = 1_000_000;
const MAX_OCCURRENCES: usize = 1_000_000;
const MAX_STRING_BYTES: usize = 1_024 * 1_024;
const MAX_RANGE_VALUES: usize = 4;

#[derive(Default)]
pub(super) struct Index {
    pub documents: Vec<Document>,
}

#[derive(Default)]
pub(super) struct Document {
    pub language: String,
    pub path: String,
    pub occurrences: Vec<Occurrence>,
    pub symbols: Vec<Symbol>,
}

#[derive(Default)]
pub(super) struct Occurrence {
    pub range: Vec<i32>,
    pub symbol: String,
    pub roles: i32,
}

#[derive(Default)]
pub(super) struct Symbol {
    pub symbol: String,
    pub display_name: String,
}

#[derive(Default)]
struct Counts {
    documents: usize,
    symbols: usize,
}

pub(super) fn decode(bytes: &[u8]) -> Result<Index, ScipError> {
    if bytes.len() > MAX_INPUT_BYTES {
        return Err(ScipError::InputTooLarge);
    }
    let mut reader = Reader::new(bytes, 0)?;
    let mut counts = Counts::default();
    let mut index = Index::default();
    while let Some((field, wire)) = reader.key()? {
        match field {
            2 => {
                counts.documents = increment(counts.documents, MAX_DOCUMENTS, "documents")?;
                let bytes = reader.message(wire)?;
                index
                    .documents
                    .push(parse_document(bytes, reader.depth + 1, &mut counts)?);
            }
            3 => {
                counts.symbols = increment(counts.symbols, MAX_SYMBOLS, "symbols")?;
                let bytes = reader.message(wire)?;
                let _ = parse_symbol(bytes, reader.depth + 1, &mut counts)?;
            }
            _ => reader.skip(field, wire)?,
        }
    }
    Ok(index)
}

fn parse_document(bytes: &[u8], depth: usize, counts: &mut Counts) -> Result<Document, ScipError> {
    let mut reader = Reader::new(bytes, depth)?;
    let mut document = Document::default();
    while let Some((field, wire)) = reader.key()? {
        match field {
            1 => document.path = reader.string(wire)?,
            2 => {
                if document.occurrences.len() >= MAX_OCCURRENCES {
                    return Err(ScipError::CountLimit("occurrences"));
                }
                let bytes = reader.message(wire)?;
                document
                    .occurrences
                    .push(parse_occurrence(bytes, depth + 1)?);
            }
            3 => {
                counts.symbols = increment(counts.symbols, MAX_SYMBOLS, "symbols")?;
                let bytes = reader.message(wire)?;
                document
                    .symbols
                    .push(parse_symbol(bytes, depth + 1, counts)?);
            }
            4 => document.language = reader.string(wire)?,
            _ => reader.skip(field, wire)?,
        }
    }
    Ok(document)
}

fn parse_occurrence(bytes: &[u8], depth: usize) -> Result<Occurrence, ScipError> {
    let mut reader = Reader::new(bytes, depth)?;
    let mut occurrence = Occurrence::default();
    while let Some((field, wire)) = reader.key()? {
        match (field, wire) {
            (1, 0) => {
                if occurrence.range.len() >= MAX_RANGE_VALUES {
                    return Err(ScipError::InvalidRange);
                }
                occurrence.range.push(reader.int32()?);
            }
            (1, 2) => {
                let packed = reader.bytes()?;
                let mut values = Reader::new(packed, depth)?;
                while !values.done() {
                    if occurrence.range.len() >= MAX_RANGE_VALUES {
                        return Err(ScipError::InvalidRange);
                    }
                    occurrence.range.push(values.int32()?);
                }
            }
            (2, _) => occurrence.symbol = reader.string(wire)?,
            (3, 0) => occurrence.roles = reader.int32()?,
            _ => reader.skip(field, wire)?,
        }
    }
    Ok(occurrence)
}

fn parse_symbol(bytes: &[u8], depth: usize, counts: &mut Counts) -> Result<Symbol, ScipError> {
    let mut reader = Reader::new(bytes, depth)?;
    let mut symbol = Symbol::default();
    while let Some((field, wire)) = reader.key()? {
        match field {
            1 => symbol.symbol = reader.string(wire)?,
            6 => symbol.display_name = reader.string(wire)?,
            7 => {
                counts.documents = increment(counts.documents, MAX_DOCUMENTS, "documents")?;
                let bytes = reader.message(wire)?;
                let _ = parse_document(bytes, depth + 1, counts)?;
            }
            _ => reader.skip(field, wire)?,
        }
    }
    Ok(symbol)
}

fn increment(value: usize, maximum: usize, name: &'static str) -> Result<usize, ScipError> {
    let value = value.checked_add(1).ok_or(ScipError::CountLimit(name))?;
    if value > maximum {
        Err(ScipError::CountLimit(name))
    } else {
        Ok(value)
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
    position: usize,
    depth: usize,
}

impl<'a> Reader<'a> {
    const fn new(bytes: &'a [u8], depth: usize) -> Result<Self, ScipError> {
        if depth > MAX_DEPTH {
            return Err(ScipError::DepthLimit);
        }
        Ok(Self {
            bytes,
            position: 0,
            depth,
        })
    }

    const fn done(&self) -> bool {
        self.position == self.bytes.len()
    }

    fn key(&mut self) -> Result<Option<(u32, u8)>, ScipError> {
        if self.done() {
            return Ok(None);
        }
        let key = self.varint()?;
        let field = u32::try_from(key >> 3).map_err(|_| ScipError::MalformedWire)?;
        let wire = u8::try_from(key & 7).map_err(|_| ScipError::MalformedWire)?;
        if field == 0 || field > 0x1fff_ffff || wire > 5 {
            return Err(ScipError::MalformedWire);
        }
        Ok(Some((field, wire)))
    }

    fn varint(&mut self) -> Result<u64, ScipError> {
        let mut value = 0_u64;
        for shift in (0..=63).step_by(7) {
            let byte = *self
                .bytes
                .get(self.position)
                .ok_or(ScipError::MalformedWire)?;
            self.position += 1;
            if shift == 63 && byte > 1 {
                return Err(ScipError::MalformedWire);
            }
            value |= u64::from(byte & 0x7f) << shift;
            if byte & 0x80 == 0 {
                return Ok(value);
            }
        }
        Err(ScipError::MalformedWire)
    }

    fn int32(&mut self) -> Result<i32, ScipError> {
        let bytes = self.varint()?.to_le_bytes();
        Ok(i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    fn message(&mut self, wire: u8) -> Result<&'a [u8], ScipError> {
        if wire != 2 {
            return Err(ScipError::MalformedWire);
        }
        self.bytes()
    }

    fn string(&mut self, wire: u8) -> Result<String, ScipError> {
        let bytes = self.message(wire)?;
        if bytes.len() > MAX_STRING_BYTES {
            return Err(ScipError::StringTooLarge);
        }
        std::str::from_utf8(bytes)
            .map(str::to_owned)
            .map_err(|_| ScipError::MalformedUtf8)
    }

    fn bytes(&mut self) -> Result<&'a [u8], ScipError> {
        let length = usize::try_from(self.varint()?).map_err(|_| ScipError::MalformedWire)?;
        let end = self
            .position
            .checked_add(length)
            .ok_or(ScipError::MalformedWire)?;
        let bytes = self
            .bytes
            .get(self.position..end)
            .ok_or(ScipError::MalformedWire)?;
        self.position = end;
        Ok(bytes)
    }

    fn take(&mut self, length: usize) -> Result<(), ScipError> {
        self.position = self
            .position
            .checked_add(length)
            .filter(|end| *end <= self.bytes.len())
            .ok_or(ScipError::MalformedWire)?;
        Ok(())
    }

    fn skip(&mut self, field: u32, wire: u8) -> Result<(), ScipError> {
        match wire {
            0 => self.varint().map(|_| ()),
            1 => self.take(8),
            2 => self.bytes().map(|_| ()),
            3 => self.skip_group(field, self.depth + 1),
            5 => self.take(4),
            _ => Err(ScipError::MalformedWire),
        }
    }

    fn skip_group(&mut self, group: u32, depth: usize) -> Result<(), ScipError> {
        if depth > MAX_DEPTH {
            return Err(ScipError::DepthLimit);
        }
        while let Some((field, wire)) = self.key()? {
            if wire == 4 {
                return if field == group {
                    Ok(())
                } else {
                    Err(ScipError::MalformedWire)
                };
            }
            if wire == 3 {
                self.skip_group(field, depth + 1)?;
            } else {
                self.skip(field, wire)?;
            }
        }
        Err(ScipError::MalformedWire)
    }
}
