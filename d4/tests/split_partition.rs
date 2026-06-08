mod common;

use common::write_test_d4;
use d4::{
    ptab::{DecodeResult, Decoder, PTablePartitionWriter, PrimaryTablePartReader},
    stab::{SecondaryTablePartReader, SecondaryTablePartWriter},
    Dictionary,
};

fn value(result: DecodeResult) -> i32 {
    match result {
        DecodeResult::Definitely(value) => value,
        DecodeResult::Maybe(value) => value,
    }
}

#[test]
fn split_keeps_record_when_partition_ends_inside_secondary_record() {
    let file = write_test_d4(
        "secondary_split",
        1000,
        Dictionary::SimpleRange { low: 0, high: 1 },
        |writer| {
            for (_primary, mut secondary) in writer.parallel_parts(None).unwrap() {
                secondary.encode_record(0, 1000, 7).unwrap();
                secondary.flush().unwrap();
                secondary.finish().unwrap();
            }
        },
    );

    let mut reader = file.open();
    let (primary, mut secondary) = reader.split(Some(20)).unwrap().remove(0);
    assert_eq!(primary.region(), ("chr1", 0, 16));

    assert_eq!(secondary.decode(0), Some(7));
    assert_eq!(secondary.decode(15), Some(7));
}

#[test]
fn decode_block_handles_unaligned_primary_table_offsets() {
    let file = write_test_d4(
        "unaligned_decode_block",
        64,
        Dictionary::SimpleRange { low: 0, high: 64 },
        |writer| {
            for (mut primary, mut secondary) in writer.parallel_parts(None).unwrap() {
                let mut encoder = primary.make_encoder();
                for pos in 0..64u32 {
                    assert!(encoder.encode(pos as usize, (pos % 16) as i32));
                }
                secondary.flush().unwrap();
                secondary.finish().unwrap();
            }
        },
    );

    let mut reader = file.open();
    let mut parts = reader.split(None).unwrap();
    assert_eq!(parts.len(), 1);

    let (mut primary, _secondary) = parts.pop().unwrap();
    let mut decoder = primary.make_decoder();
    let mut decoded = Vec::new();
    decoder.decode_block(10, 2, |pos, result| {
        decoded.push((pos, value(result)));
    });

    assert_eq!(decoded, vec![(10, 10), (11, 11)]);

    let mut decoder = primary.make_decoder();
    assert_eq!(value(Decoder::decode(&mut decoder, 11)), 11);
}
