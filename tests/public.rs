// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
use avif_parse::{ContentLightLevel, MasteringDisplayColourVolume, Error};
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
        log::debug!("parsing {:?}", path.display());
        let input = &mut File::open(path).expect("bad file");
        match avif_parse::read_avif(input) {
            Ok(avif) => {
                avif.primary_item_metadata().unwrap();
                avif.alpha_item_metadata().unwrap();
            },
            Err(Error::Unsupported(why)) => log::warn!("{why}"),
            Err(err) => {
                log::error!("{:?}: {err}", path.display());
                errors += 1;
            },
        }
    }
    assert_eq!(0, errors);
}

// Fixture files generated with avif-serialize 0.8.7 (feat/hdr-metadata branch).
// Each is a minimal AVIF container (64×64, 8-bit, profile 0) with HDR metadata properties.

#[test]
fn parse_clli() {
    let input = &mut File::open("tests/hdr-clli.avif").expect("fixture missing");
    let parsed = avif_parse::read_avif(input).expect("parse failed");

    let cll = parsed.content_light_level.expect("clli missing");
    assert_eq!(cll.max_content_light_level, 1000);
    assert_eq!(cll.max_pic_average_light_level, 400);
    assert!(parsed.mastering_display.is_none());
}

#[test]
fn parse_mdcv() {
    let input = &mut File::open("tests/hdr-mdcv.avif").expect("fixture missing");
    let parsed = avif_parse::read_avif(input).expect("parse failed");

    let mdcv = parsed.mastering_display.expect("mdcv missing");
    // BT.2020 primaries
    assert_eq!(mdcv.primaries, [(8500, 39850), (6550, 2300), (35400, 14600)]);
    // D65 white point
    assert_eq!(mdcv.white_point, (15635, 16450));
    assert_eq!(mdcv.max_luminance, 10_000_000); // 1000 cd/m²
    assert_eq!(mdcv.min_luminance, 50);          // 0.005 cd/m²
    assert!(parsed.content_light_level.is_none());
}

#[test]
fn parse_clli_and_mdcv() {
    let input = &mut File::open("tests/hdr-clli-mdcv.avif").expect("fixture missing");
    let parsed = avif_parse::read_avif(input).expect("parse failed");

    let cll = parsed.content_light_level.expect("clli missing");
    assert_eq!(cll, ContentLightLevel {
        max_content_light_level: 4000,
        max_pic_average_light_level: 1000,
    });

    let mdcv = parsed.mastering_display.expect("mdcv missing");
    assert_eq!(mdcv, MasteringDisplayColourVolume {
        primaries: [(8500, 39850), (6550, 2300), (35400, 14600)],
        white_point: (15635, 16450),
        max_luminance: 40_000_000, // 4000 cd/m²
        min_luminance: 50,
    });
}
