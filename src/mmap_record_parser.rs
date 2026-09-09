use std::fs::OpenOptions;
use std::io;

use memmap2::MmapMut;

use crate::record::Record;

const HEADER_COLUMNS: [&str; 4] = ["type", "client", "tx", "amount"];

/// Memory-maps a CSV input file (read-write, so records can be marked
/// in-place) and lets individual records be read or edited directly by
/// their byte offset, without re-parsing the whole file.
pub struct MmapRecordParser {
    mmap: MmapMut,
    // the header's columns, in the file's own order, used to map each
    // record's fields onto `Record` by name regardless of column order.
    headers: csv::ByteRecord,
    // byte offset where the first data record starts, i.e. right after
    // the header line.
    first_record_offset: usize,
    // cursor used by `next`, advanced past each record it returns.
    next_offset: usize,
}

impl MmapRecordParser {
    /// Opens `path` read-write, memory-maps it, and validates that its
    /// first line is a header row containing exactly the expected column
    /// names (`type`, `client`, `tx`, `amount`) — in any order. Follows the
    /// same validation flow as loading the file for streaming processing.
    pub fn new(path: &str) -> io::Result<Self> {
        let file = OpenOptions::new().read(true).write(true).open(path)?;
        let mmap = unsafe { MmapMut::map_mut(&file)? };

        let header_end = mmap
            .iter()
            .position(|&b| b == b'\n')
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing header row"))?;

        let header_line = mmap[..header_end]
            .strip_suffix(b"\r")
            .unwrap_or(&mmap[..header_end]);
        let header = String::from_utf8_lossy(header_line);
        let columns: Vec<&str> = header.split(',').map(str::trim).collect();

        let mut sorted_columns = columns.clone();
        sorted_columns.sort_unstable();
        let mut expected_columns = HEADER_COLUMNS;
        expected_columns.sort_unstable();

        if sorted_columns != expected_columns {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("unexpected header columns: '{header}'"),
            ));
        }

        let headers: csv::ByteRecord = columns.into_iter().collect();
        let first_record_offset = header_end + 1;

        Ok(MmapRecordParser {
            mmap,
            headers,
            first_record_offset,
            next_offset: first_record_offset,
        })
    }

    /// Byte offset where the first data record starts.
    pub fn first_record_offset(&self) -> usize {
        self.first_record_offset
    }

    /// Returns the raw bytes of the line starting at `offset` (excluding a
    /// trailing `\n`/`\r\n`) together with the offset just past it, or
    /// `None` if `offset` falls outside the mapped file.
    fn line_at(&self, offset: usize) -> Option<(&[u8], usize)> {
        let rest = self.mmap.get(offset..)?;
        let line_end = rest.iter().position(|&b| b == b'\n').unwrap_or(rest.len());
        let line = rest[..line_end]
            .strip_suffix(b"\r")
            .unwrap_or(&rest[..line_end]);

        Some((line, offset + line_end + 1))
    }

    /// Tries to read the CSV record starting at `offset`. Returns `None` if
    /// `offset` falls outside the mapped file; otherwise `Some` carries the
    /// parse result for the line found there.
    pub fn try_read_record(&self, offset: usize) -> Option<Result<Record, csv::Error>> {
        let (line, _) = self.line_at(offset)?;
        Some(Record::from_bytes(line, &self.headers))
    }

    /// Overwrites the first byte of the `type` column's cell for the record
    /// starting at `offset` with `value`, mutating the underlying file
    /// directly. Returns `false` without writing anything if `offset` is
    /// out of bounds or the `type` cell is empty.
    pub fn set_type_first_byte(&mut self, offset: usize, value: u8) -> bool {
        let Some(type_index) = self.headers.iter().position(|column| column == b"type") else {
            return false;
        };

        let field_offset = {
            let Some((line, _)) = self.line_at(offset) else {
                return false;
            };
            match field_start_offset(line, type_index) {
                Some(field_offset) if field_offset < line.len() => field_offset,
                _ => return false,
            }
        };

        self.mmap[offset + field_offset] = value;
        true
    }

    /// Returns the next record and the offset it was read from, advancing
    /// past it, or `None` once the end of the file is reached. Blank lines
    /// (including a trailing one left by a final newline) are skipped.
    ///
    /// The returned offset can be passed straight to `set_type_first_byte`
    /// to mark the record just read while continuing to traverse the file.
    pub fn next_record(&mut self) -> Option<(usize, Result<Record, csv::Error>)> {
        loop {
            let offset = self.next_offset;
            let (line, next_offset) = self.line_at(offset)?;

            if line.is_empty() {
                self.next_offset = next_offset;
                continue;
            }

            let result = Record::from_bytes(line, &self.headers);
            self.next_offset = next_offset;
            return Some((offset, result));
        }
    }
}

/// Returns the byte offset (relative to `line`) where the `field_index`-th
/// comma-separated field begins, or `None` if `line` has fewer fields.
fn field_start_offset(line: &[u8], field_index: usize) -> Option<usize> {
    if field_index == 0 {
        return Some(0);
    }

    let mut fields_seen = 0;
    for (i, &b) in line.iter().enumerate() {
        if b == b',' {
            fields_seen += 1;
            if fields_seen == field_index {
                return Some(i + 1);
            }
        }
    }

    None
}
