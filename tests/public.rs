// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
use std::fs::File;
use std::path::Path;

static IMAGE_AVIF: &str = "av1-avif/testFiles/Microsoft/Monochrome.avif";
static IMAGE_AVIF_EXTENTS: &str = "tests/kodim-extents.avif";
static IMAGE_AVIF_CORRUPT: &str = "tests/bug-1655846.avif";
static IMAGE_AVIF_CORRUPT_2: &str = "tests/bug-1661347.avif";
static IMAGE_AVIF_GRID: &str = "av1-avif/testFiles/Microsoft/Summer_in_Tomsk_720p_5x4_grid.avif";
static AVIF_TEST_DIR: &str = "av1-avif/testFiles";

#[test]
fn public_avif_primary_item() {
    let context = &mut avif_parse::AvifContext::new();
    let input = &mut File::open(IMAGE_AVIF).expect("Unknown file");
    avif_parse::read_avif(input, context).expect("read_avif failed");
    assert_eq!(context.primary_item.len(), 6979);
    assert_eq!(context.primary_item[0..4], [0x12, 0x00, 0x0a, 0x0a]);
}

#[test]
fn public_avif_primary_item_split_extents() {
    let context = &mut avif_parse::AvifContext::new();
    let input = &mut File::open(IMAGE_AVIF_EXTENTS).expect("Unknown file");
    avif_parse::read_avif(input, context).expect("read_avif failed");
    assert_eq!(context.primary_item.len(), 4387);
}

#[test]
fn public_avif_bug_1655846() {
    let context = &mut avif_parse::AvifContext::new();
    let input = &mut File::open(IMAGE_AVIF_CORRUPT).expect("Unknown file");
    assert!(avif_parse::read_avif(input, context).is_err());
}

#[test]
fn public_avif_bug_1661347() {
    let context = &mut avif_parse::AvifContext::new();
    let input = &mut File::open(IMAGE_AVIF_CORRUPT_2).expect("Unknown file");
    assert!(avif_parse::read_avif(input, context).is_err());
}

#[test]
fn public_avif_read_samples() {
    env_logger::init();

    for entry in walkdir::WalkDir::new(AVIF_TEST_DIR) {
        let entry = entry.expect("AVIF entry");
        let path = entry.path();
        if !path.is_file() || path.extension().unwrap_or_default() != "avif" {
            eprintln!("Skipping {:?}", path);
            continue; // Skip directories, ReadMe.txt, etc.
        }
        if path == Path::new(IMAGE_AVIF_GRID) {
            eprintln!("Skipping {:?}", path);
            continue; // Remove when public_avif_primary_item_is_grid passes
        }
        println!("parsing {:?}", path);
        let context = &mut avif_parse::AvifContext::new();
        let input = &mut File::open(path).expect("Unknow file");
        avif_parse::read_avif(input, context).expect("read_avif failed");
    }
}
