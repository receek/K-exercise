use std::collections::HashMap;
use std::fs::File;
use std::io;

use memmap2::MmapMut;
use tempfile::tempfile;

use crate::client::{Client, ClientError};
use crate::mmap_record_parser::MmapRecordParser;
use crate::transaction::Transaction;

pub type Index = usize;

// 2^32 different client ids
const INDEX_COUNT: u64 = 1 << 32;
pub const INDEX_SIZE: usize = std::mem::size_of::<Index>();
const TOTAL_SIZE: u64 = INDEX_COUNT * (INDEX_SIZE as u64);

// Backs the transaction index with a memory-mapped temporary file, so large
// input files don't need their index kept fully in memory.
pub struct IndexBuffer {
    _file: File,
    mmap: MmapMut,
    // byte length of `file`/`mmap`, used to bounds-check `key` in `get`/`set`
    size: u64,
}

impl IndexBuffer {
    fn new() -> io::Result<Self> {
        let file = tempfile()?;
        file.set_len(TOTAL_SIZE)?;
        let mmap = unsafe { MmapMut::map_mut(&file)? };

        Ok(IndexBuffer {
            _file: file,
            mmap,
            size: TOTAL_SIZE,
        })
    }

    /// Decodes the little-endian `Index` value stored for `key`, or `None`
    /// if `key` falls outside the buffer.
    pub fn get(&self, key: u32) -> Option<Index> {
        let offset = key as u64 * INDEX_SIZE as u64;
        if offset + INDEX_SIZE as u64 > self.size {
            return None;
        }

        let offset = offset as usize;
        let bytes: [u8; INDEX_SIZE] = self.mmap[offset..offset + INDEX_SIZE].try_into().unwrap();
        Some(Index::from_le_bytes(bytes))
    }

    /// Encodes `value` as little-endian bytes at `key`'s position. Returns
    /// `false` without writing anything if `key` falls outside the buffer.
    pub fn set(&mut self, key: u32, value: Index) -> bool {
        let offset = key as u64 * INDEX_SIZE as u64;
        if offset + INDEX_SIZE as u64 > self.size {
            return false;
        }

        let offset = offset as usize;
        self.mmap[offset..offset + INDEX_SIZE].copy_from_slice(&value.to_le_bytes());
        true
    }
}

pub struct TransactionEngine {
    pub clients: HashMap<u16, Client>,
    index: IndexBuffer,
}

impl TransactionEngine {
    pub fn new() -> io::Result<Self> {
        Ok(TransactionEngine {
            clients: HashMap::new(),
            index: IndexBuffer::new()?,
        })
    }

    pub fn process_transaction(
        &mut self,
        transaction: Transaction,
        tx_offset: Index,
        input_file: &mut MmapRecordParser,
    ) -> Result<(), ClientError> {
        let client_id = transaction.client_id();
        let client = self
            .clients
            .entry(client_id)
            .or_insert_with(|| Client::new(client_id));
        client.process_transaction(transaction, tx_offset, &mut self.index, input_file)
    }
}
