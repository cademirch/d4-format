use d4::{Chrom, D4FileBuilder, D4FileWriter, D4TrackReader, Dictionary};
use std::path::{Path, PathBuf};

pub struct TestD4File(PathBuf);

impl TestD4File {
    pub fn open(&self) -> D4TrackReader {
        D4TrackReader::open_first_track(self.path()).unwrap()
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestD4File {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

pub fn write_test_d4(
    name: &str,
    size: usize,
    dictionary: Dictionary,
    write_parts: impl FnOnce(&mut D4FileWriter),
) -> TestD4File {
    let mut path = std::env::temp_dir();
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    path.push(format!(
        "d4_test_{}_{}_{}.d4",
        name,
        std::process::id(),
        nonce
    ));

    let mut builder = D4FileBuilder::new(&path);
    builder.append_chrom(std::iter::once(Chrom {
        name: "chr1".to_string(),
        size,
    }));
    builder.set_dictionary(dictionary);

    let mut writer: D4FileWriter = builder.create().unwrap();
    write_parts(&mut writer);
    drop(writer);

    TestD4File(path)
}
