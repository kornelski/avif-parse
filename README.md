# AVIF file structure parser

Get AV1 payload and alpha channel metadata out of AVIF image files. The parser is a fork of Mozilla's MP4 parser used in Firefox, so it's designed to be robust and safely handle untrusted data.

The parser is compatible with files supported by libavif, Chrome 85 and Firefox 81a.

[API documentation](https://docs.rs/avif-parse/)

This crate doesn't include an AV1 decoder. To display the pixels you will additionally need [dav1d](https://code.videolan.org/videolan/dav1d) or [libaom](//lib.rs/libaom-sys).
