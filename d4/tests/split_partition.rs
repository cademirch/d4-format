//! Regression tests for two bugs that surface when reading a D4 file via the
//! `local_reader` (mmap) path with `split(Some(N))` + `decode_block`.
//!
//! 1. `SparseArrayReader::split` previously dropped records for any partition
//!    whose start coincided with a stab record's start but whose end fell inside
//!    that same record. The partition then had no records and `stab.decode`
//!    returned `None` for every position in it.
//!
//! 2. `PrimaryTableCodec::{decode, decode_block}` did unaligned `u32` reads
//!    via `&*(ptr as *const u32)`, which is UB and panics in debug builds on
//!    aarch64 (`misaligned pointer dereference`).

use d4::{
    ptab::{DecodeResult, Decoder, PTablePartitionWriter, PrimaryTablePartReader},
    stab::{SecondaryTablePartReader, SecondaryTablePartWriter},
    Chrom, D4FileBuilder, D4FileWriter, D4TrackReader, Dictionary,
};
use std::path::PathBuf;

fn temp_d4_path(name: &str) -> PathBuf {
    let mut p = std::env::temp_dir();
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    p.push(format!("d4_split_test_{}_{}_{}.d4", name, std::process::id(), nonce));
    p
}

#[test]
fn split_keeps_record_when_partition_is_inside_first_stab_record() {
    let path = temp_d4_path("inside_first_record");
    let _ = std::fs::remove_file(&path);

    // bit_width = 0 dictionary so any non-default value lands in the stab.
    {
        let mut builder = D4FileBuilder::new(&path);
        builder.append_chrom(std::iter::once(Chrom {
            name: "chr1".to_string(),
            size: 1000,
        }));
        builder.set_dictionary(Dictionary::SimpleRange { low: 0, high: 1 });
        let mut writer: D4FileWriter = builder.create().unwrap();
        let parts = writer.parallel_parts(None).unwrap();
        for (_pt, mut st) in parts {
            st.encode_record(0, 1000, 7).unwrap();
            st.flush().unwrap();
            st.finish().unwrap();
        }
    }

    // span=20 rounds down to 16; partition[0] = [0..16) is fully inside the
    // single stab record [0..1000), which is what tripped the bug.
    let mut reader: D4TrackReader = D4TrackReader::open_first_track(&path).unwrap();
    let parts = reader.split(Some(20)).unwrap();

    let mut saw_first_partition = false;
    for (pt, mut stab) in parts {
        let (_chr, b, e) = pt.region();
        if b == 0 {
            saw_first_partition = true;
        }
        for pos in b..e.min(1000) {
            assert_eq!(
                stab.decode(pos),
                Some(7),
                "stab.decode returned wrong value in partition [{b}..{e}) at pos {pos}",
            );
        }
    }
    assert!(saw_first_partition, "expected a partition starting at 0");

    let _ = std::fs::remove_file(&path);
}

#[test]
fn decode_block_handles_unaligned_byte_offset() {
    // bit_width = 6 means most positions land on non-4-byte-aligned byte offsets.
    let path = temp_d4_path("unaligned_decode_block");
    let _ = std::fs::remove_file(&path);

    {
        let mut builder = D4FileBuilder::new(&path);
        builder.append_chrom(std::iter::once(Chrom {
            name: "chr1".to_string(),
            size: 64,
        }));
        builder.set_dictionary(Dictionary::SimpleRange { low: 0, high: 64 });
        let mut writer: D4FileWriter = builder.create().unwrap();
        let parts = writer.parallel_parts(None).unwrap();
        for (mut pt, mut st) in parts {
            let mut enc = pt.make_encoder();
            for pos in 0..64u32 {
                let v = (pos % 16) as i32;
                if !enc.encode(pos as usize, v) {
                    st.encode(pos, v).unwrap();
                }
            }
            st.flush().unwrap();
            st.finish().unwrap();
        }
    }

    let mut reader: D4TrackReader = D4TrackReader::open_first_track(&path).unwrap();
    let parts = reader.split(None).unwrap();
    for (mut pt, _stab) in parts {
        let mut codec = pt.make_decoder();
        // pos=10 with bit_width=6 → byte offset 7, which is 4-byte-misaligned;
        // the buggy code would panic here on aarch64 debug builds.
        let mut decoded = Vec::new();
        codec.decode_block(10, 6, |pos, value| {
            let v = match value {
                DecodeResult::Definitely(v) => v,
                DecodeResult::Maybe(v) => v,
            };
            decoded.push((pos, v));
        });
        assert_eq!(decoded.len(), 6);
        for (pos, v) in decoded {
            assert_eq!(v, (pos as u32 % 16) as i32, "wrong value at pos {pos}");
        }
        // Also exercise PrimaryTableCodec::decode at an unaligned offset.
        let mut codec = pt.make_decoder();
        let one = match Decoder::decode(&mut codec, 11) {
            DecodeResult::Definitely(v) => v,
            DecodeResult::Maybe(v) => v,
        };
        assert_eq!(one, (11u32 % 16) as i32);
    }

    let _ = std::fs::remove_file(&path);
}
