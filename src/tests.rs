//! Module for parsing ISO Base Media Format aka video/mp4 streams.
//! Internal unit tests.

// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use fallible_collections::TryRead as _;

use std::convert::TryInto as _;

use std::io::Read as _;

#[test]
fn read_to_end_() {
    let mut src = b"1234567890".take(5);
    let buf = src.read_into_try_vec().unwrap();
    assert_eq!(buf.len(), 5);
    assert_eq!(buf, b"12345".as_ref());
}

#[test]
fn read_to_end_oom() {
    let mut src = b"1234567890".take(std::usize::MAX.try_into().expect("usize < u64"));
    assert!(src.read_into_try_vec().is_err());
}
