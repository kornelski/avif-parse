/// Check if needed fields are still public.
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
extern crate mp4parse as mp4;

use std::convert::TryInto;
use std::fs::File;
use std::io::{Cursor, Read};
use std::path::Path;

static MINI_MP4: &str = "tests/minimal.mp4";
static MINI_MP4_WITH_METADATA: &str = "tests/metadata.mp4";
static MINI_MP4_WITH_METADATA_STD_GENRE: &str = "tests/metadata_gnre.mp4";

static AUDIO_EME_CENC_MP4: &str = "tests/bipbop-cenc-audioinit.mp4";
static VIDEO_EME_CENC_MP4: &str = "tests/bipbop_480wp_1001kbps-cenc-video-key1-init.mp4";
// The cbcs files were created via shaka-packager from Firefox's test suite's bipbop.mp4 using:
// packager-win.exe
// in=bipbop.mp4,stream=audio,init_segment=bipbop_cbcs_audio_init.mp4,segment_template=bipbop_cbcs_audio_$Number$.m4s
// in=bipbop.mp4,stream=video,init_segment=bipbop_cbcs_video_init.mp4,segment_template=bipbop_cbcs_video_$Number$.m4s
// --protection_scheme cbcs --enable_raw_key_encryption
// --keys label=:key_id=7e571d047e571d047e571d047e571d21:key=7e5744447e5744447e5744447e574421
// --iv 11223344556677889900112233445566
// --generate_static_mpd --mpd_output bipbop_cbcs.mpd
// note: only the init files are needed for these tests
static AUDIO_EME_CBCS_MP4: &str = "tests/bipbop_cbcs_audio_init.mp4";
static VIDEO_EME_CBCS_MP4: &str = "tests/bipbop_cbcs_video_init.mp4";
static VIDEO_AV1_MP4: &str = "tests/tiny_av1.mp4";
static IMAGE_AVIF: &str = "av1-avif/testFiles/Microsoft/Monochrome.avif";
static IMAGE_AVIF_EXTENTS: &str = "tests/kodim-extents.avif";
static IMAGE_AVIF_CORRUPT: &str = "tests/bug-1655846.avif";
static IMAGE_AVIF_CORRUPT_2: &str = "tests/bug-1661347.avif";
static IMAGE_AVIF_GRID: &str = "av1-avif/testFiles/Microsoft/Summer_in_Tomsk_720p_5x4_grid.avif";
static AVIF_TEST_DIR: &str = "av1-avif/testFiles";

#[test]
fn public_avif_primary_item() {
    let context = &mut mp4::AvifContext::new();
    let input = &mut File::open(IMAGE_AVIF).expect("Unknown file");
    mp4::read_avif(input, context).expect("read_avif failed");
    assert_eq!(context.primary_item.len(), 6979);
    assert_eq!(context.primary_item[0..4], [0x12, 0x00, 0x0a, 0x0a]);
}

#[test]
fn public_avif_primary_item_split_extents() {
    let context = &mut mp4::AvifContext::new();
    let input = &mut File::open(IMAGE_AVIF_EXTENTS).expect("Unknown file");
    mp4::read_avif(input, context).expect("read_avif failed");
    assert_eq!(context.primary_item.len(), 4387);
}

#[test]
fn public_avif_bug_1655846() {
    let context = &mut mp4::AvifContext::new();
    let input = &mut File::open(IMAGE_AVIF_CORRUPT).expect("Unknown file");
    assert!(mp4::read_avif(input, context).is_err());
}

#[test]
fn public_avif_bug_1661347() {
    let context = &mut mp4::AvifContext::new();
    let input = &mut File::open(IMAGE_AVIF_CORRUPT_2).expect("Unknown file");
    assert!(mp4::read_avif(input, context).is_err());
}

#[test]
#[ignore] // Remove when we add support; see https://github.com/mozilla/mp4parse-rust/issues/198
fn public_avif_primary_item_is_grid() {
    let context = &mut mp4::AvifContext::new();
    let input = &mut File::open(IMAGE_AVIF_GRID).expect("Unknown file");
    mp4::read_avif(input, context).expect("read_avif failed");
    // Add some additional checks
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
        let context = &mut mp4::AvifContext::new();
        let input = &mut File::open(path).expect("Unknow file");
        mp4::read_avif(input, context).expect("read_avif failed");
    }
}
