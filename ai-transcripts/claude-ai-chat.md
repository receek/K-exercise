# Chat Transcript: Rust CSV Parsing, mmap, and memmap2

---

## 1. Avoiding loading CSV into memory

**User:**
> I developed rust program that reads CSV file, parses it and process in some way. I used `csv` crate. The input file can be quite large so I want to avoid loading it to memory (like `csv` crate does). I am considering to mmap file into memory

**Assistant:**

A quick clarification first, because it might change what you actually need to do: the `csv` crate does **not** load the whole file into memory by default. `csv::Reader::from_reader` wraps whatever `Read` you give it (typically a `BufReader<File>`) and parses record-by-record, using an internal buffer (8 KB by default) that gets refilled as you iterate. As long as you use `.records()` / `.byte_records()` in a loop and don't do `.collect::<Vec<_>>()` on all rows, you're already streaming with roughly constant memory usage regardless of file size.

So if your program feels like it's using too much memory, it's more likely due to:
- collecting all `StringRecord`s into a `Vec` somewhere
- cloning/allocating per-field `String`s in your processing logic instead of using `csv::ByteRecord`/`StringRecord` views
- buffering results for output rather than streaming those too

Worth checking that before reaching for mmap.

### If you still want mmap

mmap doesn't reduce memory usage for **sequential** parsing — the OS still has to page file content into RAM as you read it (that's true whether you `read()` bytes into a buffer or mmap and touch the pages). The main things mmap actually buys you:

- Avoids an extra copy (kernel page cache → your buffer) since you access the page-cache-backed pages directly.
- No repeated syscalls for each read.
- The OS can lazily fault in only the pages you touch, and evict them under memory pressure — useful if the file is bigger than RAM.
- Useful if you need random access into the file, not just sequential.

Trade-offs:
- `SIGBUS` if the underlying file is truncated/modified while mapped (a `Read`-based approach just gets an I/O error instead — safer).
- CSV parsing is inherently sequential/streaming anyway, so most of the mmap benefit (avoiding a copy) is real but the *memory* benefit compared to a properly-streaming `BufReader` is minimal.

If you want to try it, `memmap2` + `csv` compose nicely because a `Mmap` derefs to `&[u8]`, which you can wrap in a `Cursor` and hand to `csv::Reader`:

```rust
use memmap2::Mmap;
use std::fs::File;
use std::io::Cursor;
use csv::ReaderBuilder;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let file = File::open("big.csv")?;
    let mmap = unsafe { Mmap::map(&file)? }; // unsafe: file must not be modified concurrently

    // Optional: hint sequential access pattern to the OS
    #[cfg(unix)]
    mmap.advise(memmap2::Advice::Sequential)?;

    let mut rdr = ReaderBuilder::new()
        .has_headers(true)
        .from_reader(Cursor::new(&mmap[..]));

    let mut record = csv::ByteRecord::new();
    while rdr.read_byte_record(&mut record)? {
        // process record fields as &[u8] without extra allocation where possible
    }

    Ok(())
}
```

Notes on this:
- `Mmap::map` is `unsafe` because Rust can't guarantee the file won't be truncated/modified by another process while mapped, which can cause a `SIGBUS` crash. If the file could be written to concurrently, this is a real risk to weigh.
- Using `read_byte_record` reuses the same `ByteRecord` buffer instead of allocating a new one per row — this matters more for throughput/allocation pressure than mmap does.
- On 64-bit systems, mapping even a huge file is fine since it just reserves virtual address space; physical memory use tracks the pages you actually touch.

### My actual recommendation

Unless you have a specific reason (huge file + need random access, or profiling shows the buffered-read copy is a bottleneck), I'd stick with `BufReader<File>` + `csv::Reader` + `ByteRecord` reuse, and just make sure your own processing code isn't materializing the whole file. It's simpler, avoids the `SIGBUS`/concurrent-modification hazard, and for sequential parsing the performance difference vs mmap is usually small.

---

## 2. Random access due to cached state

**User:**
> The program I developed sequentaly pass throug the record (from top to bottom) but some records may trigger accessing state that was processed before so I cache this state in memory. This program may need random access to `csv` file. Does `mmap`ing has sense here?

**Assistant:**

Good question — but I think there's a distinction worth pulling apart first: **are you doing random access into the file, or random access into your own in-memory cache?**

From what you described — "some records trigger accessing state that was processed before, so I cache this state in memory" — that's not file random access at all. You're building some in-memory structure (a `HashMap<Key, State>`, running aggregates, etc.) as you stream through, and later records look up into *that structure*, not back into the CSV file. mmap doesn't help with this case — you're never seeking into the file itself, so the page cache access pattern is still purely sequential (one forward pass). The random access is happening in your own data structure, in RAM, regardless of how you read the file.

So the real question for your actual memory usage is: **how big does that cache get?** If it's bounded (e.g., state per unique key, and keys are limited), you're fine. If it grows unboundedly with the number of rows, that's your memory bottleneck — not the file-reading strategy — and mmap won't fix it.

### Where mmap *would* actually make sense for you

If instead of caching full state in memory, you wanted to trade memory for I/O by **re-reading specific earlier records from the file on demand** (i.e., "when I need state from record #4128, seek back to byte offset X and re-parse just that record instead of keeping it cached"), then yes — that's genuine random access into the file, and mmap is a good fit:

- You'd keep a side index: `HashMap<Key, u64>` mapping key → byte offset of the record (much smaller than caching the whole parsed state).
- When you need old state, slice into the mmap at that offset and parse just that record.
- The OS page cache handles the "random access" efficiently — no seek() syscalls, no re-reading pages already in cache, and pages you don't touch again can be evicted under memory pressure.

```rust
use memmap2::Mmap;
use std::collections::HashMap;

// while streaming, record byte offset per key
let mut offsets: HashMap<Key, u64> = HashMap::new();

// later, to look up prior state:
fn record_at<'a>(mmap: &'a Mmap, offset: u64) -> &'a [u8] {
    let start = offset as usize;
    let end = mmap[start..].iter().position(|&b| b == b'\n').map(|i| start + i).unwrap_or(mmap.len());
    &mmap[start..end]
    // then parse this one line with csv::ReaderBuilder or a manual split
}
```

This trades **CPU (re-parsing a line)** for **memory (not caching state)** — worth it only if your cached state is large per key and/or you have far more historical lookups than you're willing to hold in RAM simultaneously.

### Bottom line

- If your cache is a modest map of "processed state per key," memory usage is bounded by cardinality of keys, not file size — mmap is irrelevant here, don't bother.
- If your cache is the actual bottleneck (huge state per key, or state you could cheaply re-derive by re-reading the record), then mmap-backed random re-access + a lightweight offset index is a reasonable trade.

---

## 3. Scaling to 2^32 records

**User:**
> Let say there could be 2^32 records

**Assistant:**

That scale changes the calculus meaningfully. With 2^32 (~4.3 billion) records, a few things stop being "lightweight":

### The offset index itself may become the bottleneck

If you're indexing **every record** (not deduplicated by key), a `HashMap<Key, u64>` with 4 billion entries costs roughly:
- 8 bytes for the `u64` offset
- + key size (say 8 bytes for a `u64` key, more for strings)
- + `HashMap` overhead (typically 1.5–3x the raw data due to load factor, buckets, tombstones)

Even in the best case (~16 bytes/entry raw), that's ~64 GB just for the index — before you've touched the actual mmap'd data. So the question becomes: **how many unique keys are there, really?** If it's genuinely up to 4 billion unique keys, no in-memory index survives this. If most records share a much smaller set of keys (e.g., a few million), you're fine — the index is bounded by key cardinality, not row count, same conclusion as before.

### If the index itself needs to be huge, mmap the index too

If you can't avoid an index approaching billions of entries, keep it **on disk as well**, memory-mapped:
- A flat, fixed-width array `[u64; N]` (offset per record, if records are indexable by ordinal position) can be a single mmap'd file — no hashmap overhead, O(1) lookup by index, and the OS pages it in/out under memory pressure just like the CSV mmap.
- If keyed lookup is needed (not just "the Nth record"), consider an on-disk hash index or a proper embedded key-value store (`sled`, `redb`, `rocksdb`) instead of hand-rolling one — these are built exactly for "billions of keys, can't fit in RAM, need random access."

### File offsets: use u64, not u32

At 2^32 records, even at a conservative ~20 bytes/record average, the file itself is ~80+ GB — well past `u32::MAX` (4 GB) for byte offsets. Make sure any offset type is `u64` (or `usize` on 64-bit), not `u32` — an easy off-by-type mistake at this scale that works fine in testing on small files and then silently wraps in production.

### mmap on 64-bit is still fine at this size

Just to confirm: mmap'ing an 80+ GB file on a 64-bit system is not a problem — it only reserves virtual address space (measured in exabytes on x86-64/aarch64), and physical RAM usage tracks only the pages you actually touch, managed by the OS page cache under normal memory pressure. So the mmap side of this doesn't need to change at 2^32 records — it's the **index** that needs rethinking, not the file access mechanism.

### Getting offsets from `csv` crate directly

Since you're already using the `csv` crate, you don't need to hand-roll offset tracking — `csv::Reader` exposes `Position` via `.position()` on the reader, or via `StringRecord::position()` / `ByteRecord::position()` if `.has_headers(false)` behavior and preserving positions is configured correctly:

```rust
let mut rdr = csv::ReaderBuilder::new().from_reader(file);
let mut record = csv::StringRecord::new();
while rdr.read_record(&mut record)? {
    if let Some(pos) = record.position() {
        let byte_offset = pos.byte(); // u64
        // store byte_offset in your index, keyed however makes sense
    }
}
```

This saves you from manually scanning for newlines to compute offsets.

### So, the real question

At this scale, everything hinges on **unique key cardinality**, not record count:
- Bounded, modest cardinality (thousands–low millions) → your original in-memory cache approach is fine regardless of 2^32 records; row count doesn't matter, only distinct-state count does.
- Cardinality also approaches billions → neither a plain in-memory cache nor a plain in-memory offset index survives; you need either an on-disk mmap'd flat index (if keys are dense/ordinal) or an embedded on-disk KV store (if keys are sparse/arbitrary), with the CSV mmap only providing the "re-fetch original record bytes" half of the picture.

---

## 4. 2^32 unique records with unique keys

**User:**
> Let's say I would have 2^32 uniqe records with uniqe keys

**Assistant:**

That's the worst case for indexing: cardinality = row count, so nothing about the earlier "bounded by unique keys" escape hatch applies anymore. Every one of the 4.3 billion records is a potential random-access target. Let's work through what's actually left standing.

### Any in-memory structure keyed by all 2^32 keys is out

Even a minimal raw array of `(key: u64, offset: u64)` pairs is 16 bytes × 2^32 ≈ **64 GB**, before any hashmap/tree overhead. A `HashMap` realistically pushes that to 100–150+ GB. Unless you have a machine with that much RAM to spare just for the index, this rules out any general in-memory keyed index.

### Option 1 — Sort by key, mmap, binary search (no index at all)

If you control the file layout (e.g., a preprocessing pass), the cleanest solution: **sort the file by key once**, mmap it, and do binary search directly on the mmap'd bytes to find a record by key.

- No separate index structure — the sorted file *is* the index.
- Binary search on 2^32 records is ~32 comparisons; each touches one page, so worst case ~32 page faults per lookup (fast, especially with SSD/NVMe or partially warm page cache).
- Requires either fixed-width records (trivial offset math: `record_size * mid`) or, for variable-width CSV, a secondary flat array of `u64` offsets (still 32 GB just for offsets — see below) or newline-scanning during binary search (works, but adds log-n scan overhead per probe).
- Cost: an O(n log n) external sort of a many-GB/TB file up front. Tools like `sort` (GNU coreutils, which does external merge sort) or writing your own chunked sort-and-merge in Rust can do this, but it's a real preprocessing step, not free.

This avoids building any auxiliary index structure bigger than the data itself — the sort is the one-time cost, and lookups afterward are cheap and memory-light.

### Option 2 — Fixed-width flat offset array, mmap'd (if keys are dense integers 0..2^32)

If your keys are already dense integers (or can be mapped to one), skip hashing entirely:

- Build a flat file: `offsets[key] = byte_offset_in_csv`, as raw `u64` entries, i.e. an array of 2^32 × 8 bytes = **32 GB** file.
- mmap *this* file too. Lookup is `O(1)`: `let offset = index_mmap[key * 8..key*8+8]` — no hashing, no tree, just array indexing.
- 32 GB fits comfortably in modern server RAM if you want it fully resident, or the OS pages it in/out like any other mmap if it doesn't.
- This is essentially what Option 1 gives you "for free" via sorting, but this approach keeps the CSV file in original/unsorted order and pays the cost in a dedicated side file instead.

### Option 3 — Don't hand-roll it: use an embedded on-disk KV store

If keys are sparse (arbitrary strings, non-contiguous integers, UUIDs, etc.) and sorting/dense-mapping isn't practical, this is exactly the problem embedded KV stores like **`sled`**, **`redb`**, or **`rocksdb`** (via `rust-rocksdb`) solve:

- They handle on-disk B-trees/LSM-trees with their own caching, far more robust than a hand-rolled mmap index.
- You pay some overhead per entry (bloom filters, block indices, etc.) but they're built and battle-tested for exactly "billions of keys, doesn't fit in RAM."
- Slower than a hand-rolled flat array (Option 2) or sorted binary search (Option 1), but much less engineering risk, and they handle updates/inserts if your data isn't static.

### Which to pick

| Situation | Best fit |
|---|---|
| Keys are dense integers (or can be remapped to them) | Option 2 — flat mmap'd offset array |
| Static dataset, one-time build, keys sortable | Option 1 — sort once, binary search on mmap |
| Sparse/arbitrary keys, want a known-good tool, less custom code | Option 3 — `sled`/`redb`/`rocksdb` |

All three make the CSV file itself just a source of record bytes accessed via mmap once you have an offset — that part of your original plan stays valid. What changes at 2^32-unique-keys scale is that the *index* has to live on disk too, not in RAM.

---

## 5. Which crate exposes an mmap interface

**User:**
> Give me rust crate that expose mmap interface

**Assistant:**

For mmap in Rust, the standard, actively-maintained crate is:

## `memmap2`

```toml
[dependencies]
memmap2 = "0.9"
```

It's the maintained fork of the original `memmap` crate (which is deprecated/unmaintained — avoid it). `memmap2` is what was used in the earlier examples.

Basic usage:

```rust
use memmap2::{Mmap, MmapOptions};
use std::fs::File;

let file = File::open("data.csv")?;
let mmap = unsafe { Mmap::map(&file)? };

// mmap derefs to &[u8]
let byte_at_100 = mmap[100];
let slice = &mmap[0..1024];
```

Key parts of the API:

- `Mmap::map(&file)` — read-only mapping.
- `MmapMut::map_mut(&file)` — writable mapping (for an offset-index file, if you go that route).
- `MmapOptions::new().offset(x).len(y).map(&file)` — map only a portion of a file, useful if you don't want to map an entire huge file at once.
- `.advise(Advice::Sequential)` / `.advise(Advice::Random)` (Unix only) — hints to the OS about your access pattern (`madvise`), which matters for this case: use `Sequential` while doing the initial streaming pass, `Random` when doing lookups by offset.
- `.flush()` / `.flush_async()` — for `MmapMut`, to control when writes are persisted.

Both `Mmap::map` and `MmapMut::map_mut` are `unsafe` — the safety contract is that you're responsible for the file not being truncated or otherwise invalidated by another process while mapped (violating this can cause `SIGBUS`).

### Alternatives worth knowing about

- **`memmap-rs`** — old name for what's now `memmap2`; don't use directly.
- **`mmap-rs`** — lower-level, more control over protection flags/guard pages, less commonly used for simple file-backed cases.
- **`positioned-io`** — not mmap, but worth knowing about: gives you `pread`/`pwrite`-style random access on a `File` without mmap at all, if you decide mmap's unsafety/SIGBUS risk isn't worth it for your use case.

---

## 6. Does memmap2 handle big files (64 GB) correctly?

**User:**
> Does `memmap2` handles big files (like 64 GB) correctly?

**Assistant:**

Yes — on a 64-bit system, `memmap2` handles 64 GB files correctly, with a few caveats worth knowing about.

### Why it works

`mmap()` (and Windows' `CreateFileMapping`/`MapViewOfFile`, which `memmap2` uses on Windows too) just reserves a range of **virtual address space** and lets the OS fault in pages from the file on demand. It doesn't allocate physical RAM for the whole file up front. On x86-64/aarch64, the virtual address space is measured in exabytes (typically 128 TB–256 TB of usable user-space range depending on OS), so mapping a 64 GB file is trivial from an address-space perspective — `memmap2` doesn't do anything special here, it's just a thin safe(ish) wrapper over the OS syscall, and the OS handles it natively.

`memmap2` internally uses `usize`/`u64` for lengths and offsets, not `u32`, so there's no truncation issue for large files on 64-bit targets.

### Where it would *not* work

- **32-bit systems**: address space is capped at 4 GB (often less, ~2–3 GB usable), so you literally cannot mmap a 64 GB file in one go — `Mmap::map` would fail. Not a `memmap2` limitation specifically, it's a hard OS/architecture constraint. If you need to support 32-bit targets, you'd have to map in chunks with `MmapOptions::offset()/len()`.
- **Available *virtual* address space vs disk space**: on 64-bit this is a non-issue for 64 GB, but worth knowing the ceiling exists in principle.

### Practical things that actually matter at 64 GB

1. **Physical RAM usage is separate from mmap size.** The OS pages in only what you touch, and evicts under pressure — so mapping a 64 GB file on a machine with, say, 16 GB RAM works fine as long as your access pattern doesn't require holding all 64 GB "hot" simultaneously (e.g., pure sequential scan or bounded working set is fine; if you truly random-access the whole thing repeatedly, you'll thrash the page cache and hit disk).
2. **Open file handles / `off_t` size support**: Rust's `std::fs::File` and the underlying syscalls (`mmap` on 64-bit Linux/macOS, `MapViewOfFile` on 64-bit Windows) all support 64-bit file offsets natively — no 2 GB/4 GB file-size ceiling like some legacy 32-bit APIs had.
3. **`Mmap::map` return errors**: it can still fail — e.g., insufficient virtual memory (rare at 64 GB on 64-bit), permission issues, or the file being empty (`mmap` of a zero-length file is an error on POSIX; `memmap2` will return `Err` in that case, worth handling explicitly).
4. **`SIGBUS` risk grows with file size in practice** — not because of `memmap2` itself, but because a 64 GB file is more likely to be on a network filesystem, more likely to be concurrently modified/truncated during a long-running job, and a longer-running program has a larger window for something to go wrong externally. Worth being deliberate about file locking or at least documenting the assumption that the file is immutable while mapped.
5. **Windows specifically**: `MapViewOfFile` under the hood also supports large files fine on 64-bit Windows, but historically had some quirks with very large mappings on certain systems/configurations — less battle-tested than Linux/macOS for huge mmaps, worth testing on your actual target OS if Windows is in scope.

### Bottom line

For this case — 64-bit target, 64 GB (or even the 80+ GB from the 2^32-records scenario) CSV file, sequential + random access — `memmap2` will handle it correctly with no special configuration needed. Just make sure you're building for a 64-bit target, and consider `.advise(Advice::Sequential)` during your initial streaming pass to get better OS readahead behavior at that scale.

---

## 7. Creating a large temp file backed by mmap, auto-removed on exit/crash

**User:**
> How to create big file (like 8bytes x 2^32) that will be linked to mmap. I want this file to be removed from filesystem after the program termination (even in case of program fail)

**Assistant:**

Good instinct — this is a well-known Unix pattern: **create the file, then unlink it immediately while keeping the file descriptor open.** POSIX guarantees the file's data stays alive as long as any process holds an open fd/mapping to it, but it disappears from the filesystem namespace immediately, and the space is reclaimed automatically when the last fd closes — including when the process crashes, is killed with `SIGKILL`, or panics, because the OS closes all fds on process exit unconditionally. No cleanup code of yours needs to run.

### Easiest: use the `tempfile` crate

It implements exactly this pattern portably (Unix unlink-after-open, Windows `FILE_FLAG_DELETE_ON_CLOSE`), so you don't have to hand-roll platform-specific fd tricks.

```toml
[dependencies]
tempfile = "3"
memmap2 = "0.9"
```

```rust
use memmap2::MmapMut;
use tempfile::tempfile;
use std::io::Result;

const RECORD_COUNT: u64 = 1 << 32; // 2^32
const RECORD_SIZE: u64 = 8;        // u64 offsets
const TOTAL_SIZE: u64 = RECORD_COUNT * RECORD_SIZE; // 32 GiB

fn main() -> Result<()> {
    // Creates a file that is already unlinked (Unix) / delete-on-close (Windows).
    let file = tempfile()?;

    // Set the file's logical size. On most filesystems this creates a
    // *sparse* file — no physical disk blocks allocated until you write.
    file.set_len(TOTAL_SIZE)?;

    let mut mmap = unsafe { MmapMut::map_mut(&file)? };

    // Now use it as a flat [u64] array, e.g.:
    let idx: usize = 12345;
    let bytes: [u8; 8] = 42u64.to_le_bytes();
    mmap[idx * 8..idx * 8 + 8].copy_from_slice(&bytes);

    Ok(())
    // `file` and `mmap` drop here; on Unix the already-unlinked inode's
    // space is freed by the kernel at this point (or immediately on crash).
}
```

### Notes on this

- **`set_len` creates a sparse file**, not physically allocated space — good for avoiding upfront I/O, but means writes later can fail with `ENOSPC` if the disk doesn't actually have 32 GB free at write time, even though the initial `set_len` succeeded. If you want to *guarantee* the space up front (avoid write-time surprises), use real allocation instead — see below.
- **Where does `tempfile()` put the file?** It uses the OS temp dir (`$TMPDIR`/`/tmp` on Unix, `%TEMP%` on Windows) by default. If that's a `tmpfs` (RAM-backed), a 32 GB file will consume actual RAM, defeating the point of using mmap-over-disk. Check whether your system's temp dir is disk-backed or RAM-backed, or explicitly control the location:

```rust
use tempfile::tempfile_in;
let file = tempfile_in("/path/on/a/real/disk")?;
```

- **Guaranteeing real disk space (avoid sparse-file surprises)**: on Linux, use `fallocate` via the `nix` crate or `rustix` instead of relying on `set_len`'s sparse behavior:

```rust
use rustix::fs::{fallocate, FallocateFlags};
use std::os::unix::io::AsFd;

fallocate(file.as_fd(), FallocateFlags::empty(), 0, TOTAL_SIZE)?;
```

  This actually reserves the blocks on disk immediately, so a later write can't fail with out-of-space partway through.

- **Why not just `File::create` + `std::fs::remove_file` right after?** That's the manual version of what `tempfile` does, and works identically on Unix — but doesn't work correctly on Windows (files there are typically locked against deletion while open, unless you pass the right creation flags), which is exactly why `tempfile` exists and handles the platform difference for you.

- **This only protects against the *file* lingering, not against losing the data on crash.** Since this is meant as a working index (not something you need to survive the crash), that's exactly the semantics you want — crash or normal exit, the 32 GB file vanishes, no orphaned disk usage.

---

## 8. More about the `tempfile` crate

**User:**
> Tell me more about this crate

**Assistant:**

`tempfile` is the de-facto standard Rust crate for secure, automatically-cleaned-up temporary files and directories. It's maintained under the Rust `tempfile` GitHub org (used internally by cargo, rustc, and a huge chunk of the ecosystem), so it's about as trustworthy a dependency as you'll find for this.

## Main types it exposes

### `tempfile()` / `tempfile_in(dir)`
Returns a plain `std::fs::File` that's anonymous from the start (created and unlinked/delete-on-close atomically as part of creation, not as two separate steps you do yourself). No path is ever exposed to your code because there's no path to hand back — this is the right tool when you never need to name the file, which matches an mmap-index use case exactly.

### `NamedTempFile` / `Builder::new().tempfile()`
Creates a temp file that **does** have a visible path (accessible via `.path()`), useful if you need to pass the path to another process, reopen it elsewhere, or use APIs that require a path rather than an fd. It's deleted when the `NamedTempFile` is dropped (`Drop` impl calls remove), *not* via the unlink-on-create trick — so unlike bare `tempfile()`, a `kill -9` on your process **will** leak this file, since there's no `Drop` run on `SIGKILL`. Worth knowing this distinction: `tempfile()` (anonymous) survives crashes cleanly; `NamedTempFile` only cleans up on graceful drop/panic-unwind.

### `TempDir`
Same idea as `NamedTempFile` but for a whole directory tree — creates a uniquely-named temp dir, recursively removes it on `Drop`. Same crash caveat as `NamedTempFile` applies (drop-based cleanup, not unlink-based).

### `Builder`
Lets you customize prefix/suffix, permissions, and number of random characters used for uniqueness, for either `NamedTempFile` or `TempDir`:

```rust
use tempfile::Builder;

let file = Builder::new()
    .prefix("myapp-index-")
    .suffix(".tmp")
    .tempfile_in("/data/scratch")?;
```

## Key properties relevant to this use case

- **Security**: filenames are generated with a cryptographically random suffix and created with `O_CREAT | O_EXCL`-equivalent semantics (or platform equivalent), avoiding race conditions/symlink attacks that a naive "generate name, then create" approach is vulnerable to. This matters less for the anonymous-file case but is part of why the crate is trusted broadly.
- **Cross-platform**: abstracts over the fact that Unix (unlink-while-open) and Windows (`FILE_FLAG_DELETE_ON_CLOSE`) have completely different mechanisms for "delete but keep usable" — you get one API regardless of target OS.
- **Feature flags**: the crate is fairly minimal by default; there's a `nightly` feature for some niche APIs, but for `tempfile()`, `tempfile_in()`, `MmapMut::map_mut` you don't need any non-default features.
- **`.into_file()` / `.reopen()` / `.as_file()`** on `NamedTempFile`: lets you get at the underlying `File` for mmap'ing, matching what you'd do with the anonymous version.

## Which one fits a 32 GB offset-index case

Stick with plain `tempfile()`/`tempfile_in()` (anonymous, no path) — since:
1. You never need to name/reopen the file from elsewhere.
2. "Removed even on program failure" — anonymous unlink-on-create is the only variant that guarantees this on `SIGKILL`/hard crash, since it doesn't depend on `Drop` running at all.

One thing worth double-checking for this case specifically: confirm `tempfile_in("/your/disk/path")` points at a real disk-backed filesystem with ≥32 GB free, since the default temp dir on many systems is small and/or `tmpfs`-backed, and a sparse `set_len` will only reveal that problem later when you actually write into pages that need real blocks.

---

## 9. Reading unwritten regions of a sparse file

**User:**
> If I read tempfile in the place that was not written that what I would get?

**Assistant:**

You'd get **zero bytes** — this is well-defined behavior, not garbage/uninitialized memory, and it holds whether you read via mmap or via normal file I/O.

### Why

When you create a file and `set_len()` it to some large size without writing, the filesystem creates a **sparse file**: it records the logical length but doesn't allocate physical disk blocks for the regions you never wrote. Any read from an unwritten region — whether through `read()`, `pread()`, or through an mmap'd page — is guaranteed by POSIX semantics to return zeros, as if the space were actually there and zero-filled. This is a filesystem-level guarantee (ext4, xfs, apfs, NTFS all support sparse files and honor this), not something `memmap2` or Rust implements itself.

So for the offset-index case:

```rust
let file = tempfile()?;
file.set_len(TOTAL_SIZE)?; // sparse, nothing written yet
let mmap = unsafe { MmapMut::map_mut(&file)? };

// reading an index slot you never wrote:
let bytes = &mmap[500_000 * 8 .. 500_000 * 8 + 8];
assert_eq!(bytes, &[0u8; 8]); // guaranteed
```

This is actually convenient: since a "record offset" of `0` is a natural sentinel meaning "not yet indexed" (assuming record 0's own valid offset isn't literally `0`, or you reserve offset 0 / use `Option`-style encoding), the sparse-zero default gives you a free "unset" marker without needing to pre-initialize the whole 32 GB array yourself.

### Two caveats worth knowing

1. **Only true holes read as zero.** If you write real data to a slot and later want to "unset" it, you'd have to explicitly write zeros back — the filesystem doesn't track "was written then zeroed" any differently from "was written non-zero," it's only genuinely-never-touched regions that are guaranteed sparse/zero.
2. **Sparse support isn't universal.** Nearly all filesystems you'd realistically use (ext4, xfs, btrfs, apfs, most Windows NTFS setups) support this, but a few edge-case/legacy or exotic filesystems don't support sparse files at all — in which case `set_len()` might actually allocate (zero-filled) blocks eagerly instead of leaving a hole, which changes your disk usage but *not* the zero-read guarantee; you'd still read zeros either way, the only difference is whether the space was reserved eagerly or lazily.

So: safe to rely on zeros for unwritten slots, and it's a nice fit for using `0` as a "not yet indexed" sentinel.

---

## 10. Decoding 8 bytes as little-endian integer

**User:**
> Let say I read 8 bytes end I want to decode them as little endian integer

**Assistant:**

Use `u64::from_le_bytes`, which takes a `[u8; 8]` array:

```rust
let bytes: [u8; 8] = mmap[offset..offset + 8].try_into().unwrap();
let value = u64::from_le_bytes(bytes);
```

`mmap[offset..offset+8]` gives you a `&[u8]` slice, and `try_into()` converts it into a fixed-size `[u8; 8]` array (fallible because the slice length isn't statically known to be 8 — it'll panic via `unwrap()` if the slice isn't exactly 8 bytes, which shouldn't happen if your indexing math is correct, but you could handle it with `?`/`expect()` with a message instead if you want a clearer error than a bare panic).

If you want to skip the intermediate array conversion, there's also a lower-level option (same thing, just inlined):

```rust
let value = u64::from_le_bytes(mmap[offset..offset + 8].try_into().unwrap());
```

Or, if you're doing this a lot and want to avoid the `try_into`/`unwrap` boilerplate everywhere, the `byteorder` crate gives you a `ReadBytesExt` trait with `read_u64::<LittleEndian>()` directly on anything implementing `Read` — but since you're indexing directly into an in-memory slice (not a `Read` stream), the `from_le_bytes` approach above is simpler and has zero extra dependency.

One thing to double check: make sure `offset` here is a **byte offset** (i.e., `index * 8`), not the raw record index — easy off-by-factor-of-8 bug if you mix those up.

---

## 11. Using mmap'd file with the CSV reader

**User:**
> Can I use mmaped file in csv reader?

**Assistant:**

Yes — since `Mmap` derefs to `&[u8]`, and `&[u8]` implements `Read` when wrapped in a `Cursor`, you can hand it straight to `csv::Reader`:

```rust
use memmap2::Mmap;
use std::fs::File;
use std::io::Cursor;
use csv::ReaderBuilder;

let file = File::open("big.csv")?;
let mmap = unsafe { Mmap::map(&file)? };

let mut rdr = ReaderBuilder::new()
    .has_headers(true)
    .from_reader(Cursor::new(&mmap[..]));

let mut record = csv::ByteRecord::new();
while rdr.read_byte_record(&mut record)? {
    // process record
}
```

### What actually happens here

`Cursor<&[u8]>` just tracks a position into the slice and implements `Read`/`Seek` over it — no copying of the whole file happens up front. `csv::Reader` then reads through that cursor with its own internal parse buffer, same as it would with a `BufReader<File>`. The difference from the file-based version is only in how bytes get from disk into your process: page faults + page cache (mmap) vs explicit `read()` syscalls into a buffer (`BufReader`). The CSV parsing logic itself is identical either way.

### Where this is actually useful

Given the earlier random-access design (offset index → seek to a specific record → parse just that record), mmap is more useful for **on-demand single-record parsing** than for the whole-file streaming pass:

```rust
fn parse_record_at(mmap: &Mmap, offset: u64) -> csv::Result<csv::StringRecord> {
    let start = offset as usize;
    // find end of this record (next newline)
    let end = mmap[start..]
        .iter()
        .position(|&b| b == b'\n')
        .map(|i| start + i)
        .unwrap_or(mmap.len());

    let mut rdr = ReaderBuilder::new()
        .has_headers(false) // just one line, no header semantics
        .from_reader(&mmap[start..end]);

    let mut record = csv::StringRecord::new();
    rdr.read_record(&mut record)?;
    Ok(record)
}
```

This avoids re-opening the file or seeking with a `File`/`BufReader` each time you need an old record — you just slice the mmap directly and parse that slice.

### Caveats to keep in mind

- **Embedded newlines in quoted fields**: naive `position(|&b| b == b'\n')` newline-finding breaks if a CSV field contains a literal `\n` inside quotes (valid per RFC 4180). If your data can have quoted multi-line fields, you can't safely find "one record" by scanning for the next `\n` — you'd need to either disallow that in your data, or do real CSV-aware scanning (e.g., feed a small window into `csv::Reader` and let it tell you where the record actually ends via `.position()`).
- **`has_headers(false)` for single-record parsing**: when parsing just one sliced-out line, you don't want the reader treating it as a header row — set this explicitly as shown above.
- **UTF-8 validity for `StringRecord`**: if you're not sure the sliced bytes are valid UTF-8 at exactly that boundary, use `ByteRecord` instead to sidestep the validation cost/failure risk.

---

## 12. Modifying the mmap'd file while the CSV parser is reading it

**User:**
> Can I modify mmaped file during reading by CSV parser?

**Assistant:**

Short answer: **technically yes, but it's risky, and there are two separate hazards to think about — memory-safety (Rust aliasing rules) and correctness (parsing consistency).**

### Hazard 1: Rust's aliasing rules

`Mmap` (read-only) can't be modified at all — you'd need `MmapMut`. But `MmapMut` derefs to `&mut [u8]`, and if you hand `csv::Reader` a `Cursor<&[u8]>` borrowed from it while *also* writing through another handle to the same `MmapMut` concurrently, you have two aliasing mutable/shared views of the same memory at once — that's undefined behavior under Rust's memory model (same rule as any other `&mut` + `&` aliasing violation), regardless of whether the actual bytes being touched overlap. The borrow checker won't necessarily catch this either, since raw memory-mapped regions can bypass some of Rust's usual guarantees depending on how you structure the code (mmap crates generally document this as a soundness caveat — `memmap2` itself notes that if you obtain a `&mut [u8]` and any other thread/handle can write to the file, you're in "trust me" territory the type system can't fully protect).

If your writer is in a different process (not just a different part of your program), this is even less protected — you're just directly mutating shared memory the OS happens to back, with no coordination unless you build it yourself (e.g., file locks, or your own synchronization protocol).

### Hazard 2: Parsing correctness, even if memory-safe

Say you avoid the aliasing issue (e.g., single-threaded, you write only to portions of the file the CSV reader has already fully consumed and won't revisit). Even then:

- `csv::Reader` buffers ahead internally — you don't have precise control over exactly how many bytes past your logical "current record" it has already read into its internal buffer. Writing to bytes just ahead of where you think the parser is could still race with what it's already buffered vs. not yet buffered.
- **Changing the length of the file** (not just overwriting bytes in place) while mmap'd is a much harder problem — a mapping's size is fixed at `mmap()` time; if the file shrinks, accessing now-invalid pages triggers `SIGBUS` (this crashes the process, it's not a catchable Rust error); if the file grows, your existing mapping doesn't automatically see the new region — you'd need to unmap and remap, which invalidates any in-flight `csv::Reader` state.
- Even same-length in-place overwrites: if you're modifying a record's bytes *after* the CSV reader has already read that region into its internal buffer but *before* it's finished handing you the fully parsed record, you could get inconsistent results depending on exact buffer boundary timing — hard to reason about and easy to get subtly wrong.

### What's actually recommended

- **If you need to write results/derived data**: write to a *separate* file (or the mmap'd offset-index file from earlier), not back into the same mmap you're reading the CSV from. Keep read and write mappings fully separate.
- **If you need to modify the CSV file itself while parsing** (e.g., in-place transformation): don't do it via a live mmap shared with an active `csv::Reader`. Instead, either:
  - Read via mmap, but buffer/queue writes to apply to a **different** output file or a **different, later-remapped** region.
  - Or do the read pass fully, close/finish that `csv::Reader`, then remap and write — i.e., serialize read-then-write phases rather than interleaving them on the same mapping.
- **If concurrent modification is inherent to your use case** (e.g., another process appends to the file while you tail it), that's a much more specialized log-tailing/streaming problem, and mmap + `csv::Reader` isn't really the right tool — you'd want to track file growth via `fstat`/`inotify` and remap or reopen carefully, treating this as append-only rather than in-place mutation.

---

## 13. Does `DeserializeRecordsIntoIter` load the whole file into memory?

**User:**
> Does `DeserializeRecordsIntoIter` also do not load whole file into memory?

**Assistant:**

Correct — `DeserializeRecordsIntoIter` (returned by `Reader::into_deserialize::<T>()`) streams the same way as `.records()`/`.byte_records()`. It doesn't load the whole file into memory.

### What it actually does per iteration

Each call to `.next()`:
1. Pulls the next raw record from the underlying reader (same internal parse buffer as always — reads more from the source `Read`/mmap slice only as needed).
2. Deserializes that one record into your target type `T` via `serde`.
3. Yields `Result<T, csv::Error>`.

So memory use per iteration is roughly: one buffered chunk of raw bytes (small, reused) + one instance of `T` (whatever size your struct is) — not the whole file, and not all records at once.

```rust
#[derive(Deserialize)]
struct Row {
    id: u64,
    name: String,
    value: f64,
}

let mut rdr = csv::Reader::from_reader(file);
for result in rdr.into_deserialize::<Row>() {
    let row: Row = result?;
    // process row; row is dropped/reused each iteration
}
```

### Where memory *can* creep in here, specific to deserializing

- **Owned `String`/`Vec` fields in `T`**: unlike `ByteRecord`/`StringRecord` (which you can reuse via `read_byte_record`/`read_record` into the same buffer), `into_deserialize()` allocates a fresh `T` each iteration — if `T` has `String` fields, that's a new heap allocation per row per field, freed when `row` drops at the end of the loop body. This is normal streaming behavior, but if you accidentally `collect::<Vec<Row>>()` the iterator instead of processing row-by-row, you're back to holding everything in memory — same footgun as with `.records()`.
- **Borrowed deserialization (`Deserialize<'de>` with `&'de str` fields)** is possible with `csv` via `StringRecord::deserialize` for zero-copy field access, but `into_deserialize()`'s convenience iterator specifically requires `T: DeserializeOwned`, so you don't get borrowed/zero-copy fields through this particular API — if allocation-per-row-per-field is a bottleneck at 2^32 rows, that's a case for the lower-level `read_record` + manual field access/parsing instead of the deserialize iterator.

So: same constant-ish memory profile as `.records()`, as long as you iterate and process one row at a time rather than collecting.
