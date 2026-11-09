#![cfg(feature = "native")]
use hyz_things::application::question_bank::document::{complete_ids, parse};
use hyz_things::application::question_bank::images::rewrite_assets;
use std::path::{Path, PathBuf};

fn visit(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if entry.file_type().unwrap().is_dir() {
            visit(&path, out);
        } else if path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("申论-") && name.ends_with(".md"))
        {
            out.push(path);
        }
    }
}

#[test]
fn generated_shenlun_papers_parse_with_closed_images() {
    let bank = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .join("question_bank");
    let mut files = Vec::new();
    visit(&bank.join("papers"), &mut files);
    assert!(!files.is_empty(), "no generated shenlun papers");
    let mut sequence = 0u64;
    for file in files {
        let relative = file
            .strip_prefix(&bank)
            .unwrap()
            .to_str()
            .unwrap()
            .to_owned();
        let text = std::fs::read_to_string(&file).unwrap();
        let (completed, _) = complete_ids(&text, || {
            sequence += 1;
            format!("019a3d8c-7f23-7b6e-927d-{sequence:012x}")
        })
        .unwrap_or_else(|error| panic!("{relative}: {error}"));
        let (_, assets) = rewrite_assets(&completed, &relative)
            .unwrap_or_else(|error| panic!("{relative}: {error:?}"));
        for asset in assets {
            assert!(
                bank.join("assets").join(&asset).is_file(),
                "{relative}: missing asset {asset}"
            );
        }
        let paper = parse(&completed).unwrap_or_else(|error| panic!("{relative}: {error}"));
        assert_eq!(paper.view().subject, "shenlun", "{relative}");
    }
}
