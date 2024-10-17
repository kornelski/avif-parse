// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
use avif_parse::Error;
use std::fs::File;

static IMAGE_AVIF: &str = "av1-avif/testFiles/Microsoft/Monochrome.avif";
static IMAGE_AVIF_EXTENTS: &str = "tests/kodim-extents.avif";
static IMAGE_AVIF_CORRUPT: &str = "tests/bug-1655846.avif";
static IMAGE_AVIF_CORRUPT_2: &str = "tests/bug-1661347.avif";
static AOMEDIA_TEST_FILES: &str = "av1-avif/testFiles";
static LINK_U_SAMPLES: &str = "link-u-samples";

#[test]
fn public_avif_primary_item() {
    let input = &mut File::open(IMAGE_AVIF).expect("Unknown file");
    let context = avif_parse::read_avif(input).expect("read_avif failed");
    assert_eq!(context.primary_item.len(), 6979);
    assert_eq!(context.primary_item[0..4], [0x12, 0x00, 0x0a, 0x0a]);
}

#[test]
fn public_avif_primary_item_split_extents() {
    let input = &mut File::open(IMAGE_AVIF_EXTENTS).expect("Unknown file");
    let context = avif_parse::read_avif(input).expect("read_avif failed");
    assert_eq!(context.primary_item.len(), 4387);
}

#[test]
fn public_avif_bug_1655846() {
    let input = &mut File::open(IMAGE_AVIF_CORRUPT).expect("Unknown file");
    assert!(avif_parse::read_avif(input).is_err());
}

#[test]
fn public_avif_bug_1661347() {
    let input = &mut File::open(IMAGE_AVIF_CORRUPT_2).expect("Unknown file");
    assert!(avif_parse::read_avif(input).is_err());
}

#[test]
fn aomedia_sample_images() {
    test_dir(AOMEDIA_TEST_FILES);
}

#[test]
fn linku_sample_images() {
    test_dir(LINK_U_SAMPLES);
}

fn test_dir(dir: &str) {
    let _ = env_logger::builder().is_test(true).filter_level(log::LevelFilter::max()).try_init();
    let mut errors = 0;

    for entry in walkdir::WalkDir::new(dir) {
        let entry = entry.expect("AVIF entry");
        let path = entry.path();
        if !path.is_file() || path.extension().unwrap_or_default() != "avif" {
            continue; // Skip directories, ReadMe.txt, etc.
        }
        log::debug!("parsing {path:?}");
        let input = &mut File::open(path).expect("bad file");
        match avif_parse::read_avif(input) {
            Ok(avif) => {
                avif.primary_item_metadata().unwrap();
                avif.alpha_item_metadata().unwrap();
            },
            Err(Error::Unsupported(why)) => log::warn!("{why}"),
            Err(err) => {
                log::error!("{path:?}: {err}");
                errors += 1;
            },
        }
    }
    assert_eq!(0, errors);
}
