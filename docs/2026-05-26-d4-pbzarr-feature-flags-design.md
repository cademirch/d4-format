# d4 cargo feature surface for pbzarr-rs

Date: 2026-05-26
Scope: `cademirch/d4-format` fork only. The downstream consumer is
`pbzarr-rs`. Goal is to give pbzarr a dep line that pulls in exactly the
local-file read path used by pyd4's `load_values_to_buffer` —
`d4::D4TrackReader::split` + `decode_block` + `stab.decode` — and
nothing else.

Chosen approach: **rename `reader_only` → `local_reader`**. No new
module gating. The today's `reader_only` build already excludes
d4-hts, rayon, rand, and reqwest; the only thing this change does is
make the feature name match how the consumer thinks about it ("I want
the local mmap reader"). The streaming `ssio` module and the `index`
module continue to compile under this feature — they're unused dead
code for pbzarr, but pruning them would mean cordoning two `pub mod`
declarations in `lib.rs` and re-gating `task` + `http_reader`, which
the user explicitly traded off for speed.

---

## 1. Feature surface pbzarr needs

After the swap to the mapped-IO `D4TrackReader`, pbzarr's d4 consumer
file (`pbzarr/src/io/d4.rs`) will touch exactly this surface:

**Opening a track (top-level `d4::` re-exports from `lib.rs`):**
- `d4::Header` — only for `.chrom_list()` returning `&[Chrom]`.
- `d4::Chrom` — `.name: String`, `.size: usize`.
- `d4::D4TrackReader` (the mmap reader from `d4::d4file::reader`, with
  default generics `<BitArrayReader, SparseArrayReader<RangeRecord>>`).
- `D4TrackReader::open_first_track(path)` — opens the first track in a
  single-track file.
- `D4TrackReader::open_track_with_path(file_path, track_path)` —
  opens a specific track by name, if pbzarr ever needs multi-track
  files. Either of these is the entry point; pbzarr does *not* need
  `open(track_spec)` because pbzarr does its own path parsing.
- `D4TrackReader::header()` → `&Header`.
- `D4TrackReader::split(Some(chunk_size))` →
  `Vec<(P::Partition, S::Partition)>`, the per-partition pairs the
  worker threads pull off the channel.

**Per-chunk decoding (inside each worker):**
- `d4::ptab::PrimaryTablePartReader::region()` → `(&str, u32, u32)`.
- `d4::ptab::BitArrayPartReader::to_codec()` → `BitArrayDecoder`
  (`PrimaryTableCodec<bit_array::Reader>`). This is an inherent method
  on the bit_array partition reader, not on the `PrimaryTablePartReader`
  trait, so pbzarr's code is parameterised on the concrete bit_array
  partition type (which is the default `P::Partition` for
  `D4TrackReader<BitArrayReader, _>`).
- `d4::ptab::Decoder::decode_block(pos, count, |pos, DecodeResult| ...)` —
  the trait method that streams decoded values back to a closure.
- `d4::ptab::DecodeResult::{Definitely, Maybe}` — the two-variant enum
  the decoder yields; `Maybe(v)` means consult the secondary table.
- `d4::stab::SecondaryTablePartReader::decode(pos: u32) -> Option<i32>` —
  trait method used to resolve `Maybe` results.

**Forking for parallel workers:**
- The plan is N worker threads each holding its own
  `D4TrackReader::open_first_track(path)`-derived reader (one open per
  worker, then `split()` once inside the worker). pbzarr does not call
  `ValueReader::fork()` on the streaming `ssio` reader after the swap —
  that was the old path.

### What pbzarr does NOT need

These all sit behind features pbzarr already turns off. Calling them
out explicitly so the gating story is unambiguous:

- `d4::ssio::*` (streaming `Read+Seek` reader, `D4TrackReader<R>`,
  `D4TrackView`, `HttpReader`) — pbzarr is moving off this path.
- `d4::ssio::http` — gated by `http_reader`.
- `d4::task::*` (`TaskContext`, `Histogram`, `Mean`, `Sum`, …) — gated
  by `task`.
- `d4::index::*` (`D4IndexCollection`, `SecondaryFrameIndex`,
  `DataIndexRef`, `Sum`) — unconditional but unused by pbzarr.
- `d4file::D4FileBuilder`, `D4FileWriter`, `D4FileMerger`,
  `D4FileWriterExt` — gated by `writer`.
- `d4_hts::*` (BAM/CRAM input), the bam→d4 path in
  `d4::dict::Dictionary::*` — gated by `depth_profiler`.
- `d4-bigwig`, `d4tools`, `d4binding`, `pyd4` — separate crates in the
  workspace, not pulled in by depending on the `d4` crate at all.

### Where each piece comes from (gating mechanism)

| Surface pbzarr uses                 | Gated by              | Mechanism |
|-------------------------------------|------------------------|-----------|
| `Header`, `Chrom`, `Dictionary`    | always-on              | inline in `lib.rs` |
| `pub mod ptab` + `BitArrayReader`, `BitArrayDecoder`, `DecodeResult`, `Decoder` trait | `mapped_io`            | `#[cfg(feature="mapped_io")]` on `pub mod ptab` in `lib.rs:6` |
| `pub mod stab` trait defs           | always-on              | inline in `lib.rs:8` |
| `stab::SparseArrayReader`, `SparseArrayPartReader` | `mapped_io`            | `#[cfg(feature="mapped_io")]` on the `mapped` re-export in `stab/mod.rs:94` and on `mod writer` in `stab/sparse_array/mod.rs:6` |
| `D4TrackReader` (mmap), `find_tracks`, `find_tracks_in_file` | `mapped_io`            | `#[cfg(feature="mapped_io")]` on `d4file::reader` mod + the `mapped` re-export in `d4file/mod.rs:21–36` |

The `mapped_io` feature transitively enables `d4-framefile/mapped_io`,
which is what provides the `memmap` dep.

### Surface that stays compiled but unused under `local_reader`

These are inline modules in d4's source with no optional-dep
dependency, so they continue to compile even with
`default-features = false, features = ["local_reader"]`:

- `pub mod ssio` (`lib.rs:12`) — the `Read+Seek` streaming reader.
  Without `http_reader`, the HTTP submodule is gated off, but
  `ssio::reader`, `ssio::table`, `ssio::view` still compile. Imports
  `crate::index::*` unconditionally.
- `pub mod index` (`lib.rs:14`) — `D4IndexCollection`,
  `SecondaryFrameIndex`, `DataIndexRef`, `DataSummary`, `Sum`. Pure
  Rust, no optional deps.

These are wasted compile time for pbzarr but require no extra code at
runtime and pull in no extra crate deps. Pruning them is left as
future work — see "Approach we did not take" below.

---

## 2. Proposed feature additions / changes

Single change: **rename `reader_only` to `local_reader`** in
`d4/Cargo.toml`. The semantics are identical (`local_reader =
["mapped_io"]`). No other features need to change. No source files
need new `#[cfg]` cordons.

| Feature        | Status   | Gates                                  | Implies      | In `default`? |
|----------------|----------|----------------------------------------|--------------|---------------|
| `local_reader` | **new**  | nothing additional beyond `mapped_io` | `mapped_io`  | no            |
| `mapped_io`    | existing | `pub mod ptab`, `d4file::reader`, `stab::mapped::*` re-exports, `stab::sparse_array::writer` mod | `d4-framefile/mapped_io` | no |
| `writer`       | existing | `D4FileBuilder`, `D4FileWriter`, `D4FileMerger`, encode side of `ptab`/`stab` traits, writer impls | `mapped_io` | yes |
| `task`         | existing | `pub mod task`, `D4MatrixReader`, `MultiTrackReader`, parts of `index::data_index` | `mapped_io`, `rayon` | yes |
| `depth_profiler` | existing | bam→d4 paths in `dict.rs` and `d4file/writer.rs` | `d4-hts`, `rayon`, `rand` | yes |
| `http_reader`  | existing | `ssio::http`, HTTP-specific impls in `ssio::reader` | `reqwest`    | yes |
| `seq-task`     | existing | (per source: no observed `#[cfg]` references — appears dead, but leaving in place) | — | no |
| `reader_only`  | **removed** | (was alias for `mapped_io`) | — | — |

Trade-off accepted: pbzarr's `local_reader` build still contains the
unused `ssio` and `index` modules (~600 lines of dead code at compile
time, no runtime cost, no extra deps). This was an explicit choice for
speed of landing the rename over the more invasive gating change.

---

## 3. Cargo.toml diff

### `d4/Cargo.toml`

```diff
 [features]
-default = ["depth_profiler", "task", "writer", "http_reader"]
-reader_only = ["mapped_io"]
+default = ["depth_profiler", "task", "writer", "http_reader"]
+local_reader = ["mapped_io"]
 mapped_io = ["d4-framefile/mapped_io"]
 writer = ["mapped_io"]
 task = ["mapped_io", "rayon"]
 depth_profiler = ["d4-hts", "rayon", "rand"]
 http_reader = ["reqwest"]
 seq-task = []
```

No other lines change. No changes to `d4-framefile/Cargo.toml`,
`d4-hts/Cargo.toml`, or any source file.

### `pbzarr-rs`'s new dep line

In `pbzarr/Cargo.toml`:

```diff
 [dependencies.d4]
 git = "https://github.com/cademirch/d4-format.git"
 default-features = false
-features = ["reader_only"]
+features = ["local_reader"]
```

(Branch / rev pinning unchanged.) After the rename lands in the fork
and pbzarr updates its dep, `cargo build -p pbzarr` pulls in exactly:
`d4` + `d4-framefile/mapped_io` (→ `memmap`) + serde + flate2 +
smallvec + log. No d4-hts, no rayon, no reqwest, no rand.

---

## Approach we did not take

For completeness — the more aggressive option, in case we want to
revisit later:

Add `streaming_reader = []` and cordon `pub mod ssio;` and
`pub mod index;` in `lib.rs` behind it. `http_reader` would require
`streaming_reader`; `task` would also require `streaming_reader`
because `index/data_index/data.rs` has `task`-gated impls that reach
into `index`. Put `streaming_reader` in `default` so existing
consumers stay green. Then pbzarr's `local_reader` build skips
~600 lines of ssio/index source.

This was rejected for now: the savings are real but small, and the
two-line `#[cfg]` change in `lib.rs` would need to be matched by
careful re-verification that `task`, `http_reader`, and the default
build all still compile and that nothing else in d4's source reaches
into `index` without a guard. If a future pbzarr build wants to slim
the dep further, this is the obvious next step.
