use std::path::PathBuf;

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");

pub(crate) fn source_path(name: &str) -> PathBuf {
    PathBuf::from(format!("{FIXTURES}/source/{name}.srt"))
}

pub(crate) fn source(name: &str) -> Vec<u8> {
    std::fs::read(source_path(name)).expect("fixture source exists")
}

pub(crate) fn expected(name: &str) -> String {
    std::fs::read_to_string(format!("{FIXTURES}/expected/{name}.txt"))
        .expect("fixture expected text exists")
}
