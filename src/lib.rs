//! Module for parsing ISO Base Media Format aka video/mp4 streams.

// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#[macro_use]
extern crate log;

use bitreader::BitReader;
use byteorder::ReadBytesExt;
use fallible_collections::TryClone;
use fallible_collections::TryRead;
use std::convert::{TryFrom, TryInto as _};

use std::io::{Read, Take};
use std::ops::{Range, RangeFrom};

#[macro_use]
mod macros;

mod boxes;
use crate::boxes::{BoxType, FourCC};

/// This crate can be used from C.
pub mod c_api;

// Unit tests.
#[cfg(test)]
mod tests;

// Arbitrary buffer size limit used for raw read_bufs on a box.
// const BUF_SIZE_LIMIT: u64 = 10 * 1024 * 1024;

/// A trait to indicate a type can be infallibly converted to `u64`.
/// This should only be implemented for infallible conversions, so only unsigned types are valid.
trait ToU64 {
    fn to_u64(self) -> u64;
}

/// Statically verify that the platform `usize` can fit within a `u64`.
/// If the size won't fit on the given platform, this will fail at compile time, but if a type
/// which can fail TryInto<usize> is used, it may panic.
impl ToU64 for usize {
    fn to_u64(self) -> u64 {
        static_assertions::const_assert!(
            std::mem::size_of::<usize>() <= std::mem::size_of::<u64>()
        );
        self.try_into().expect("usize -> u64 conversion failed")
    }
}

/// A trait to indicate a type can be infallibly converted to `usize`.
/// This should only be implemented for infallible conversions, so only unsigned types are valid.
pub(crate) trait ToUsize {
    fn to_usize(self) -> usize;
}

/// Statically verify that the given type can fit within a `usize`.
/// If the size won't fit on the given platform, this will fail at compile time, but if a type
/// which can fail TryInto<usize> is used, it may panic.
macro_rules! impl_to_usize_from {
    ( $from_type:ty ) => {
        impl ToUsize for $from_type {
            fn to_usize(self) -> usize {
                static_assertions::const_assert!(
                    std::mem::size_of::<$from_type>() <= std::mem::size_of::<usize>()
                );
                self.try_into().expect(concat!(
                    stringify!($from_type),
                    " -> usize conversion failed"
                ))
            }
        }
    };
}

impl_to_usize_from!(u8);
impl_to_usize_from!(u16);
impl_to_usize_from!(u32);

/// Indicate the current offset (i.e., bytes already read) in a reader
trait Offset {
    fn offset(&self) -> u64;
}

/// Wraps a reader to track the current offset
struct OffsetReader<'a, T> {
    reader: &'a mut T,
    offset: u64,
}

impl<'a, T> OffsetReader<'a, T> {
    fn new(reader: &'a mut T) -> Self {
        Self { reader, offset: 0 }
    }
}

impl<'a, T> Offset for OffsetReader<'a, T> {
    fn offset(&self) -> u64 {
        self.offset
    }
}

impl<'a, T: Read> Read for OffsetReader<'a, T> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let bytes_read = self.reader.read(buf)?;
        self.offset = self
            .offset
            .checked_add(bytes_read.to_u64())
            .expect("total bytes read too large for offset type");
        Ok(bytes_read)
    }
}

pub type TryVec<T> = fallible_collections::TryVec<T>;
pub type TryString = fallible_collections::TryVec<u8>;
pub type TryHashMap<K, V> = fallible_collections::TryHashMap<K, V>;
pub type TryBox<T> = fallible_collections::TryBox<T>;

// To ensure we don't use stdlib allocating types by accident
#[allow(dead_code)]
struct Vec;
#[allow(dead_code)]
struct Box;
#[allow(dead_code)]
struct HashMap;
#[allow(dead_code)]
struct String;

/// Describes parser failures.
///
/// This enum wraps the standard `io::Error` type, unified with
/// our own parser error states and those of crates we use.
#[derive(Debug)]
pub enum Error {
    /// Parse error caused by corrupt or malformed data.
    InvalidData(&'static str),
    /// Parse error caused by limited parser support rather than invalid data.
    Unsupported(&'static str),
    /// Reflect `std::io::ErrorKind::UnexpectedEof` for short data.
    UnexpectedEOF,
    /// Propagate underlying errors from `std::io`.
    Io(std::io::Error),
    /// read_mp4 terminated without detecting a moov box.
    NoMoov,
    /// Out of memory
    OutOfMemory,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl std::error::Error for Error {}

impl From<bitreader::BitReaderError> for Error {
    fn from(_: bitreader::BitReaderError) -> Error {
        Error::InvalidData("invalid data")
    }
}

impl From<std::io::Error> for Error {
    fn from(err: std::io::Error) -> Error {
        match err.kind() {
            std::io::ErrorKind::UnexpectedEof => Error::UnexpectedEOF,
            _ => Error::Io(err),
        }
    }
}

impl From<std::string::FromUtf8Error> for Error {
    fn from(_: std::string::FromUtf8Error) -> Error {
        Error::InvalidData("invalid utf8")
    }
}

impl From<std::num::TryFromIntError> for Error {
    fn from(_: std::num::TryFromIntError) -> Error {
        Error::Unsupported("integer conversion failed")
    }
}

impl From<Error> for std::io::Error {
    fn from(err: Error) -> Self {
        let kind = match err {
            Error::InvalidData(_) => std::io::ErrorKind::InvalidData,
            Error::UnexpectedEOF => std::io::ErrorKind::UnexpectedEof,
            Error::Io(io_err) => return io_err,
            _ => std::io::ErrorKind::Other,
        };
        Self::new(kind, err)
    }
}

impl From<fallible_collections::TryReserveError> for Error {
    fn from(_: fallible_collections::TryReserveError) -> Error {
        Error::OutOfMemory
    }
}

/// Result shorthand using our Error enum.
pub(crate) type Result<T> = std::result::Result<T, Error>;

/// Basic ISO box structure.
///
/// mp4 files are a sequence of possibly-nested 'box' structures.  Each box
/// begins with a header describing the length of the box's data and a
/// four-byte box type which identifies the type of the box. Together these
/// are enough to interpret the contents of that section of the file.
///
/// See ISO 14496-12:2015 § 4.2
#[derive(Debug, Clone, Copy)]
struct BoxHeader {
    /// Box type.
    name: BoxType,
    /// Size of the box in bytes.
    size: u64,
    /// Offset to the start of the contained data (or header size).
    offset: u64,
    /// Uuid for extended type.
    uuid: Option<[u8; 16]>,
}

impl BoxHeader {
    const MIN_SIZE: u64 = 8; // 4-byte size + 4-byte type
    const MIN_LARGE_SIZE: u64 = 16; // 4-byte size + 4-byte type + 16-byte size
}

/// File type box 'ftyp'.
#[derive(Debug)]
struct FileTypeBox {
    major_brand: FourCC,
    minor_version: u32,
    compatible_brands: TryVec<FourCC>,
}

// Handler reference box 'hdlr'
#[derive(Debug)]
struct HandlerBox {
    handler_type: FourCC,
}

#[derive(Debug)]
pub(crate) struct AV1ConfigBox {
    pub(crate) profile: u8,
    pub(crate) level: u8,
    pub(crate) tier: u8,
    pub(crate) bit_depth: u8,
    pub(crate) monochrome: bool,
    pub(crate) chroma_subsampling_x: u8,
    pub(crate) chroma_subsampling_y: u8,
    pub(crate) chroma_sample_position: u8,
    pub(crate) initial_presentation_delay_present: bool,
    pub(crate) initial_presentation_delay_minus_one: u8,
    pub(crate) config_obus: TryVec<u8>,
}

#[derive(Debug, Default)]
pub struct AvifData {
    /// AV1 data for the color channels.
    ///
    /// The collected data indicated by the `pitm` box, See ISO 14496-12:2015 § 8.11.4
    pub primary_item: TryVec<u8>,
    /// AV1 data for alpha channel.
    ///
    /// Associated alpha channel for the primary item, if any
    pub alpha_item: Option<TryVec<u8>>,
    /// If true, divide RGB values by the alpha value.
    ///
    /// See `prem` in MIAF § 7.3.5.2
    pub premultiplied_alpha: bool,
}

struct AvifMeta {
    item_references: TryVec<ItemReferenceEntry>,
    properties: TryVec<AssociatedProperty>,
    primary_item_id: u32,
    iloc_items: TryVec<ItemLocationBoxItem>,
}

/// A Media Data Box
/// See ISO 14496-12:2015 § 8.1.1
struct MediaDataBox {
    /// Offset of `data` from the beginning of the file. See ConstructionMethod::File
    offset: u64,
    data: TryVec<u8>,
}

impl MediaDataBox {
    /// Check whether the beginning of `extent` is within the bounds of the `MediaDataBox`.
    /// We assume extents to not cross box boundaries. If so, this will cause an error
    /// in `read_extent`.
    fn contains_extent(&self, extent: &ExtentRange) -> bool {
        if self.offset <= extent.start() {
            let start_offset = extent.start() - self.offset;
            start_offset < self.data.len().to_u64()
        } else {
            false
        }
    }

    /// Check whether `extent` covers the `MediaDataBox` exactly.
    fn matches_extent(&self, extent: &ExtentRange) -> bool {
        if self.offset == extent.start() {
            match extent {
                ExtentRange::WithLength(range) => {
                    if let Some(end) = self.offset.checked_add(self.data.len().to_u64()) {
                        end == range.end
                    } else {
                        false
                    }
                }
                ExtentRange::ToEnd(_) => true,
            }
        } else {
            false
        }
    }

    /// Copy the range specified by `extent` to the end of `buf` or return an error if the range
    /// is not fully contained within `MediaDataBox`.
    fn read_extent(&mut self, extent: &ExtentRange, buf: &mut TryVec<u8>) -> Result<()> {
        let start_offset = extent
            .start()
            .checked_sub(self.offset)
            .expect("mdat does not contain extent");
        let slice = match extent {
            ExtentRange::WithLength(range) => {
                let range_len = range
                    .end
                    .checked_sub(range.start)
                    .expect("range start > end");
                let end = start_offset
                    .checked_add(range_len)
                    .expect("extent end overflow");
                self.data.get(start_offset.try_into()?..end.try_into()?)
            }
            ExtentRange::ToEnd(_) => self.data.get(start_offset.try_into()?..),
        };
        let slice = slice.ok_or(Error::InvalidData("extent crosses box boundary"))?;
        buf.extend_from_slice(slice)?;
        Ok(())
    }
}

/// Used for 'infe' boxes within 'iinf' boxes
/// See ISO 14496-12:2015 § 8.11.6
/// Only versions {2, 3} are supported
#[derive(Debug)]
struct ItemInfoEntry {
    item_id: u32,
    item_type: u32,
}

/// See ISO 14496-12:2015 § 8.11.12
#[derive(Debug)]
struct ItemReferenceEntry {
    item_type: FourCC,
    from_item_id: u32,
    to_item_id: u32,
}

/// Potential sizes (in bytes) of variable-sized fields of the 'iloc' box
/// See ISO 14496-12:2015 § 8.11.3
#[derive(Debug)]
enum IlocFieldSize {
    Zero,
    Four,
    Eight,
}

impl IlocFieldSize {
    fn to_bits(&self) -> u8 {
        match self {
            IlocFieldSize::Zero => 0,
            IlocFieldSize::Four => 32,
            IlocFieldSize::Eight => 64,
        }
    }
}

impl TryFrom<u8> for IlocFieldSize {
    type Error = Error;

    fn try_from(value: u8) -> Result<Self> {
        match value {
            0 => Ok(Self::Zero),
            4 => Ok(Self::Four),
            8 => Ok(Self::Eight),
            _ => Err(Error::InvalidData("value must be in the set {0, 4, 8}")),
        }
    }
}

#[derive(PartialEq)]
enum IlocVersion {
    Zero,
    One,
    Two,
}

impl TryFrom<u8> for IlocVersion {
    type Error = Error;

    fn try_from(value: u8) -> Result<Self> {
        match value {
            0 => Ok(Self::Zero),
            1 => Ok(Self::One),
            2 => Ok(Self::Two),
            _ => Err(Error::Unsupported("unsupported version in 'iloc' box")),
        }
    }
}

/// Used for 'iloc' boxes
/// See ISO 14496-12:2015 § 8.11.3
/// `base_offset` is omitted since it is integrated into the ranges in `extents`
/// `data_reference_index` is omitted, since only 0 (i.e., this file) is supported
#[derive(Debug)]
struct ItemLocationBoxItem {
    item_id: u32,
    construction_method: ConstructionMethod,
    /// Unused for ConstructionMethod::Idat
    extents: TryVec<ItemLocationBoxExtent>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum ConstructionMethod {
    File,
    Idat,
    #[allow(dead_code)] // TODO: see https://github.com/mozilla/mp4parse-rust/issues/196
    Item,
}

/// `extent_index` is omitted since it's only used for ConstructionMethod::Item which
/// is currently not implemented.
#[derive(Clone, Debug)]
struct ItemLocationBoxExtent {
    extent_range: ExtentRange,
}

#[derive(Clone, Debug)]
enum ExtentRange {
    WithLength(Range<u64>),
    ToEnd(RangeFrom<u64>),
}

impl ExtentRange {
    fn start(&self) -> u64 {
        match self {
            Self::WithLength(r) => r.start,
            Self::ToEnd(r) => r.start,
        }
    }
}

/// See ISO 14496-12:2015 § 4.2
struct BMFFBox<'a, T> {
    head: BoxHeader,
    content: Take<&'a mut T>,
}

struct BoxIter<'a, T> {
    src: &'a mut T,
}

impl<'a, T: Read> BoxIter<'a, T> {
    fn new(src: &mut T) -> BoxIter<'_, T> {
        BoxIter { src }
    }

    fn next_box(&mut self) -> Result<Option<BMFFBox<'_, T>>> {
        let r = read_box_header(self.src);
        match r {
            Ok(h) => Ok(Some(BMFFBox {
                head: h,
                content: self.src.take(h.size - h.offset),
            })),
            Err(Error::UnexpectedEOF) => Ok(None),
            Err(e) => Err(e),
        }
    }
}

impl<'a, T: Read> Read for BMFFBox<'a, T> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.content.read(buf)
    }
}

impl<'a, T: Read> TryRead for BMFFBox<'a, T> {
    fn try_read_to_end(&mut self, buf: &mut TryVec<u8>) -> std::io::Result<usize> {
        fallible_collections::try_read_up_to(self, self.bytes_left(), buf)
    }
}

impl<'a, T: Offset> Offset for BMFFBox<'a, T> {
    fn offset(&self) -> u64 {
        self.content.get_ref().offset()
    }
}

impl<'a, T: Read> BMFFBox<'a, T> {
    fn bytes_left(&self) -> u64 {
        self.content.limit()
    }

    fn get_header(&self) -> &BoxHeader {
        &self.head
    }

    fn box_iter<'b>(&'b mut self) -> BoxIter<'_, BMFFBox<'a, T>> {
        BoxIter::new(self)
    }
}

impl<'a, T> Drop for BMFFBox<'a, T> {
    fn drop(&mut self) {
        if self.content.limit() > 0 {
            let name: FourCC = From::from(self.head.name);
            debug!("Dropping {} bytes in '{}'", self.content.limit(), name);
        }
    }
}

/// Read and parse a box header.
///
/// Call this first to determine the type of a particular mp4 box
/// and its length. Used internally for dispatching to specific
/// parsers for the internal content, or to get the length to
/// skip unknown or uninteresting boxes.
///
/// See ISO 14496-12:2015 § 4.2
fn read_box_header<T: ReadBytesExt>(src: &mut T) -> Result<BoxHeader> {
    let size32 = be_u32(src)?;
    let name = BoxType::from(be_u32(src)?);
    let size = match size32 {
        // valid only for top-level box and indicates it's the last box in the file.  usually mdat.
        0 => return Err(Error::Unsupported("unknown sized box")),
        1 => {
            let size64 = be_u64(src)?;
            if size64 < BoxHeader::MIN_LARGE_SIZE {
                return Err(Error::InvalidData("malformed wide size"));
            }
            size64
        }
        _ => {
            if u64::from(size32) < BoxHeader::MIN_SIZE {
                return Err(Error::InvalidData("malformed size"));
            }
            u64::from(size32)
        }
    };
    let mut offset = match size32 {
        1 => BoxHeader::MIN_LARGE_SIZE,
        _ => BoxHeader::MIN_SIZE,
    };
    let uuid = if name == BoxType::UuidBox {
        if size >= offset + 16 {
            let mut buffer = [0u8; 16];
            let count = src.read(&mut buffer)?;
            offset += count.to_u64();
            if count == 16 {
                Some(buffer)
            } else {
                debug!("malformed uuid (short read), skipping");
                None
            }
        } else {
            debug!("malformed uuid, skipping");
            None
        }
    } else {
        None
    };
    assert!(offset <= size);
    Ok(BoxHeader {
        name,
        size,
        offset,
        uuid,
    })
}

/// Parse the extra header fields for a full box.
fn read_fullbox_extra<T: ReadBytesExt>(src: &mut T) -> Result<(u8, u32)> {
    let version = src.read_u8()?;
    let flags_a = src.read_u8()?;
    let flags_b = src.read_u8()?;
    let flags_c = src.read_u8()?;
    Ok((
        version,
        u32::from(flags_a) << 16 | u32::from(flags_b) << 8 | u32::from(flags_c),
    ))
}

// Parse the extra fields for a full box whose flag fields must be zero.
fn read_fullbox_version_no_flags<T: ReadBytesExt>(src: &mut T) -> Result<u8> {
    let (version, flags) = read_fullbox_extra(src)?;

    if flags != 0 {
        return Err(Error::Unsupported("expected flags to be 0"));
    }

    Ok(version)
}

/// Skip over the entire contents of a box.
fn skip_box_content<T: Read>(src: &mut BMFFBox<'_, T>) -> Result<()> {
    // Skip the contents of unknown chunks.
    let to_skip = {
        let header = src.get_header();
        debug!("{:?} (skipped)", header);
        header
            .size
            .checked_sub(header.offset)
            .expect("header offset > size")
    };
    assert_eq!(to_skip, src.bytes_left());
    skip(src, to_skip)
}

/// Skip over the remain data of a box.
fn skip_box_remain<T: Read>(src: &mut BMFFBox<'_, T>) -> Result<()> {
    let remain = {
        let header = src.get_header();
        let len = src.bytes_left();
        debug!("remain {} (skipped) in {:?}", len, header);
        len
    };
    skip(src, remain)
}

/// Read the contents of an AVIF file
///
/// Metadata is accumulated and returned in [`AvifData`] struct,
pub fn read_avif<T: Read>(f: &mut T) -> Result<AvifData> {
    let mut f = OffsetReader::new(f);

    let mut iter = BoxIter::new(&mut f);

    // 'ftyp' box must occur first; see ISO 14496-12:2015 § 4.3.1
    if let Some(mut b) = iter.next_box()? {
        if b.head.name == BoxType::FileTypeBox {
            let ftyp = read_ftyp(&mut b)?;
            if ftyp.major_brand != b"avif" {
                return Err(Error::InvalidData("ftyp must be 'avif'"));
            }
        } else {
            return Err(Error::InvalidData("'ftyp' box must occur first"));
        }
    }

    let mut meta = None;
    let mut mdats = TryVec::new();

    while let Some(mut b) = iter.next_box()? {
        match b.head.name {
            BoxType::MetadataBox => {
                if meta.is_some() {
                    return Err(Error::InvalidData(
                        "There should be zero or one meta boxes per ISO 14496-12:2015 § 8.11.1.1",
                    ));
                }
                meta = Some(read_avif_meta(&mut b)?);
            }
            BoxType::MediaDataBox => {
                if b.bytes_left() > 0 {
                    let offset = b.offset();
                    let data = b.read_into_try_vec()?;
                    mdats.push(MediaDataBox { offset, data })?;
                }
            }
            _ => skip_box_content(&mut b)?,
        }

        check_parser_state!(b.content);
    }

    let meta = meta.ok_or(Error::InvalidData("missing meta"))?;

    let alpha_item_id = meta
        .item_references
        .iter()
        // Auxiliary image for the primary image
        .filter(|iref| {
            iref.to_item_id == meta.primary_item_id
                && iref.from_item_id != meta.primary_item_id
                && iref.item_type == b"auxl"
        })
        .map(|iref| iref.from_item_id)
        // which has the alpha property
        .find(|&item_id| {
            meta.properties.iter().any(|prop| {
                prop.item_id == item_id
                    && match &prop.property {
                        ImageProperty::AuxiliaryType(urn) => {
                            urn.as_slice()
                                == "urn:mpeg:mpegB:cicp:systems:auxiliary:alpha".as_bytes()
                        }
                        _ => false,
                    }
            })
        });

    let mut context = AvifData::default();
    context.premultiplied_alpha = alpha_item_id.map_or(false, |alpha_item_id| {
        meta.item_references.iter().any(|iref| {
            iref.from_item_id == meta.primary_item_id
                && iref.to_item_id == alpha_item_id
                && iref.item_type == b"prem"
        })
    });

    // load data of relevant items
    for loc in meta.iloc_items.iter() {
        let mut item_data = if loc.item_id == meta.primary_item_id {
            &mut context.primary_item
        } else if Some(loc.item_id) == alpha_item_id {
            context.alpha_item.get_or_insert_with(TryVec::new)
        } else {
            continue;
        };

        if loc.construction_method != ConstructionMethod::File {
            return Err(Error::Unsupported("unsupported construction_method"));
        }
        for extent in loc.extents.iter() {
            let mut found = false;
            // try to find an overlapping mdat
            for mdat in mdats.iter_mut() {
                if mdat.matches_extent(&extent.extent_range) {
                    item_data.append(&mut mdat.data)?;
                    found = true;
                    break;
                } else if mdat.contains_extent(&extent.extent_range) {
                    mdat.read_extent(&extent.extent_range, &mut item_data)?;
                    found = true;
                    break;
                }
            }
            if !found {
                return Err(Error::InvalidData(
                    "iloc contains an extent that is not in mdat",
                ));
            }
        }
    }

    Ok(context)
}

/// Parse a metadata box in the context of an AVIF
/// Currently requires the primary item to be an av01 item type and generates
/// an error otherwise.
/// See ISO 14496-12:2015 § 8.11.1
fn read_avif_meta<T: Read + Offset>(src: &mut BMFFBox<'_, T>) -> Result<AvifMeta> {
    let version = read_fullbox_version_no_flags(src)?;

    if version != 0 {
        return Err(Error::Unsupported("unsupported meta version"));
    }

    let mut primary_item_id = None;
    let mut item_infos = None;
    let mut iloc_items = None;
    let mut item_references = TryVec::new();
    let mut properties = TryVec::new();

    let mut iter = src.box_iter();
    while let Some(mut b) = iter.next_box()? {
        match b.head.name {
            BoxType::ItemInfoBox => {
                if item_infos.is_some() {
                    return Err(Error::InvalidData(
                        "There should be zero or one iinf boxes per ISO 14496-12:2015 § 8.11.6.1",
                    ));
                }
                item_infos = Some(read_iinf(&mut b)?);
            }
            BoxType::ItemLocationBox => {
                if iloc_items.is_some() {
                    return Err(Error::InvalidData(
                        "There should be zero or one iloc boxes per ISO 14496-12:2015 § 8.11.3.1",
                    ));
                }
                iloc_items = Some(read_iloc(&mut b)?);
            }
            BoxType::PrimaryItemBox => {
                if primary_item_id.is_some() {
                    return Err(Error::InvalidData(
                        "There should be zero or one iloc boxes per ISO 14496-12:2015 § 8.11.4.1",
                    ));
                }
                primary_item_id = Some(read_pitm(&mut b)?);
            }
            BoxType::ImageReferenceBox => {
                item_references.append(&mut read_iref(&mut b)?)?;
            }
            BoxType::ImagePropertiesBox => {
                properties = read_iprp(&mut b)?;
            }
            _ => skip_box_content(&mut b)?,
        }

        check_parser_state!(b.content);
    }

    let primary_item_id = primary_item_id.ok_or(Error::InvalidData(
        "Required pitm box not present in meta box",
    ))?;

    let item_infos = item_infos.ok_or(Error::InvalidData("iinf missing"))?;

    if let Some(item_info) = item_infos.iter().find(|x| x.item_id == primary_item_id) {
        if &item_info.item_type.to_be_bytes() != b"av01" {
            warn!("primary_item_id type: {}", U32BE(item_info.item_type));
            return Err(Error::InvalidData("primary_item_id type is not av01"));
        }
    } else {
        return Err(Error::InvalidData(
            "primary_item_id not present in iinf box",
        ));
    }

    Ok(AvifMeta {
        properties,
        item_references,
        primary_item_id,
        iloc_items: iloc_items.ok_or(Error::InvalidData("iloc missing"))?,
    })
}

/// Parse a Primary Item Box
/// See ISO 14496-12:2015 § 8.11.4
fn read_pitm<T: Read>(src: &mut BMFFBox<'_, T>) -> Result<u32> {
    let version = read_fullbox_version_no_flags(src)?;

    let item_id = match version {
        0 => be_u16(src)?.into(),
        1 => be_u32(src)?,
        _ => return Err(Error::Unsupported("unsupported pitm version")),
    };

    Ok(item_id)
}

/// Parse an Item Information Box
/// See ISO 14496-12:2015 § 8.11.6
fn read_iinf<T: Read>(src: &mut BMFFBox<'_, T>) -> Result<TryVec<ItemInfoEntry>> {
    let version = read_fullbox_version_no_flags(src)?;

    match version {
        0 | 1 => (),
        _ => return Err(Error::Unsupported("unsupported iinf version")),
    }

    let entry_count = if version == 0 {
        be_u16(src)?.to_usize()
    } else {
        be_u32(src)?.to_usize()
    };
    let mut item_infos = TryVec::with_capacity(entry_count)?;

    let mut iter = src.box_iter();
    while let Some(mut b) = iter.next_box()? {
        if b.head.name != BoxType::ItemInfoEntry {
            return Err(Error::InvalidData(
                "iinf box should contain only infe boxes",
            ));
        }

        item_infos.push(read_infe(&mut b)?)?;

        check_parser_state!(b.content);
    }

    Ok(item_infos)
}

/// A simple wrapper to interpret a u32 as a 4-byte string in big-endian
/// order without requiring any allocation.
struct U32BE(u32);

impl std::fmt::Display for U32BE {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match std::str::from_utf8(&self.0.to_be_bytes()) {
            Ok(s) => f.write_str(s),
            Err(_) => write!(f, "{:x?}", self.0),
        }
    }
}

/// Parse an Item Info Entry
/// See ISO 14496-12:2015 § 8.11.6.2
fn read_infe<T: Read>(src: &mut BMFFBox<'_, T>) -> Result<ItemInfoEntry> {
    // According to the standard, it seems the flags field should be 0, but
    // at least one sample AVIF image has a nonzero value.
    let (version, _) = read_fullbox_extra(src)?;

    // mif1 brand (see ISO 23008-12:2017 § 10.2.1) only requires v2 and 3
    let item_id = match version {
        2 => be_u16(src)?.into(),
        3 => be_u32(src)?,
        _ => return Err(Error::Unsupported("unsupported version in 'infe' box")),
    };

    let item_protection_index = be_u16(src)?;

    if item_protection_index != 0 {
        return Err(Error::Unsupported(
            "protected items (infe.item_protection_index != 0) are not supported",
        ));
    }

    let item_type = be_u32(src)?;
    debug!("infe item_id {} item_type: {}", item_id, U32BE(item_type));

    // There are some additional fields here, but they're not of interest to us
    skip_box_remain(src)?;

    Ok(ItemInfoEntry { item_id, item_type })
}

fn read_iref<T: Read>(src: &mut BMFFBox<'_, T>) -> Result<TryVec<ItemReferenceEntry>> {
    let mut entries = TryVec::new();
    let version = read_fullbox_version_no_flags(src)?;
    if version > 1 {
        return Err(Error::Unsupported("iref version"));
    }

    let mut iter = src.box_iter();
    while let Some(mut b) = iter.next_box()? {
        let from_item_id = if version == 0 {
            be_u16(&mut b)? as u32
        } else {
            be_u32(&mut b)?
        };
        let item_count = be_u16(&mut b)?;
        for _ in 0..item_count {
            let to_item_id = if version == 0 {
                be_u16(&mut b)?.into()
            } else {
                be_u32(&mut b)?
            };
            entries.push(ItemReferenceEntry {
                item_type: b.head.name.into(),
                from_item_id,
                to_item_id,
            })?;
        }
    }
    Ok(entries)
}

fn read_iprp<T: Read>(src: &mut BMFFBox<'_, T>) -> Result<TryVec<AssociatedProperty>> {
    let mut iter = src.box_iter();
    let mut properties = TryVec::new();
    let mut associations = TryVec::new();

    while let Some(mut b) = iter.next_box()? {
        match b.head.name {
            BoxType::ItemPropertyContainerBox => {
                properties = read_ipco(&mut b)?;
            }
            BoxType::ItemPropertyAssociationBox => {
                associations = read_ipma(&mut b)?;
            }
            _ => return Err(Error::InvalidData("unexpected ipco child")),
        }
    }

    let mut associated = TryVec::new();
    for a in associations {
        let index = match a.property_index {
            0 => continue,
            x => x as usize - 1,
        };
        if let Some(prop) = properties.get(index) {
            if *prop != ImageProperty::Unsupported {
                associated.push(AssociatedProperty {
                    item_id: a.item_id,
                    property: prop.clone()?,
                })?;
            }
        }
    }
    Ok(associated)
}

#[derive(Debug, PartialEq)]
pub(crate) enum ImageProperty {
    Channels(TryVec<u8>),
    AuxiliaryType(TryString),
    Unsupported,
}

impl ImageProperty {
    fn clone(&self) -> Result<Self> {
        Ok(match self {
            Self::Channels(val) => Self::Channels(val.try_clone()?),
            Self::AuxiliaryType(val) => Self::AuxiliaryType(val.try_clone()?),
            Self::Unsupported => Self::Unsupported,
        })
    }
}

struct Association {
    item_id: u32,
    property_index: u16,
}

pub(crate) struct AssociatedProperty {
    pub(crate) item_id: u32,
    pub(crate) property: ImageProperty,
}

fn read_ipma<T: Read>(src: &mut BMFFBox<'_, T>) -> Result<TryVec<Association>> {
    let (version, flags) = read_fullbox_extra(src)?;

    let mut associations = TryVec::new();

    let entry_count = be_u32(src)?;
    for _ in 0..entry_count {
        let item_id = if version == 0 {
            be_u16(src)?.into()
        } else {
            be_u32(src)?
        };
        let association_count = src.read_u8()?;
        for _ in 0..association_count {
            let first_byte = src.read_u8()?;
            let essential_flag = first_byte & 1 << 7;
            let value = first_byte - essential_flag;
            let property_index = if flags & 1 != 0 {
                ((value as u16) << 8) | src.read_u8()? as u16
            } else {
                value as u16
            };
            associations.push(Association {
                item_id,
                property_index,
            })?;
        }
    }
    Ok(associations)
}

fn read_ipco<T: Read>(src: &mut BMFFBox<'_, T>) -> Result<TryVec<ImageProperty>> {
    let mut properties = TryVec::new();

    let mut iter = src.box_iter();
    while let Some(mut b) = iter.next_box()? {
        // Must push for every property to have correct index for them
        properties.push(match b.head.name {
            BoxType::PixelInformationBox => ImageProperty::Channels(read_pixi(&mut b)?),
            BoxType::AuxiliaryTypeProperty => ImageProperty::AuxiliaryType(read_auxc(&mut b)?),
            _ => {
                skip_box_remain(&mut b)?;
                ImageProperty::Unsupported
            }
        })?;
    }
    Ok(properties)
}

fn read_pixi<T: Read>(src: &mut BMFFBox<'_, T>) -> Result<TryVec<u8>> {
    let version = read_fullbox_version_no_flags(src)?;
    if version != 0 {
        return Err(Error::Unsupported("pixi version"));
    }

    let num_channels = src.read_u8()?.into();
    let mut channels = TryVec::with_capacity(num_channels)?;
    let num_channels_read = src.try_read_to_end(&mut channels)?;

    if num_channels_read != num_channels.into() {
        return Err(Error::InvalidData("invalid num_channels"));
    }

    check_parser_state!(src.content);
    Ok(channels)
}

fn read_auxc<T: Read>(src: &mut BMFFBox<'_, T>) -> Result<TryString> {
    let version = read_fullbox_version_no_flags(src)?;
    if version != 0 {
        return Err(Error::Unsupported("auxC version"));
    }

    let mut aux = TryString::new();
    loop {
        match src.read_u8()? {
            0 => break,
            c => aux.push(c)?,
        }
    }
    Ok(aux)
}

/// Parse an item location box inside a meta box
/// See ISO 14496-12:2015 § 8.11.3
fn read_iloc<T: Read>(src: &mut BMFFBox<'_, T>) -> Result<TryVec<ItemLocationBoxItem>> {
    let version: IlocVersion = read_fullbox_version_no_flags(src)?.try_into()?;

    let iloc = src.read_into_try_vec()?;
    let mut iloc = BitReader::new(&iloc);

    let offset_size: IlocFieldSize = iloc.read_u8(4)?.try_into()?;
    let length_size: IlocFieldSize = iloc.read_u8(4)?.try_into()?;
    let base_offset_size: IlocFieldSize = iloc.read_u8(4)?.try_into()?;

    let index_size: Option<IlocFieldSize> = match version {
        IlocVersion::One | IlocVersion::Two => Some(iloc.read_u8(4)?.try_into()?),
        IlocVersion::Zero => {
            let _reserved = iloc.read_u8(4)?;
            None
        }
    };

    let item_count = match version {
        IlocVersion::Zero | IlocVersion::One => iloc.read_u32(16)?,
        IlocVersion::Two => iloc.read_u32(32)?,
    };

    let mut items = TryVec::with_capacity(item_count.to_usize())?;

    for _ in 0..item_count {
        let item_id = match version {
            IlocVersion::Zero | IlocVersion::One => iloc.read_u32(16)?,
            IlocVersion::Two => iloc.read_u32(32)?,
        };

        // The spec isn't entirely clear how an `iloc` should be interpreted for version 0,
        // which has no `construction_method` field. It does say:
        // "For maximum compatibility, version 0 of this box should be used in preference to
        //  version 1 with `construction_method==0`, or version 2 when possible."
        // We take this to imply version 0 can be interpreted as using file offsets.
        let construction_method = match version {
            IlocVersion::Zero => ConstructionMethod::File,
            IlocVersion::One | IlocVersion::Two => {
                let _reserved = iloc.read_u16(12)?;
                match iloc.read_u16(4)? {
                    0 => ConstructionMethod::File,
                    1 => ConstructionMethod::Idat,
                    2 => return Err(Error::Unsupported("construction_method 'item_offset' is not supported")),
                    _ => return Err(Error::InvalidData("construction_method is taken from the set 0, 1 or 2 per ISO 14496-12:2015 § 8.11.3.3"))
                }
            }
        };

        let data_reference_index = iloc.read_u16(16)?;

        if data_reference_index != 0 {
            return Err(Error::Unsupported(
                "external file references (iloc.data_reference_index != 0) are not supported",
            ));
        }

        let base_offset = iloc.read_u64(base_offset_size.to_bits())?;
        let extent_count = iloc.read_u16(16)?;

        if extent_count < 1 {
            return Err(Error::InvalidData(
                "extent_count must have a value 1 or greater per ISO 14496-12:2015 § 8.11.3.3",
            ));
        }

        let mut extents = TryVec::with_capacity(extent_count.to_usize())?;

        for _ in 0..extent_count {
            // Parsed but currently ignored, see `ItemLocationBoxExtent`
            let _extent_index = match &index_size {
                None | Some(IlocFieldSize::Zero) => None,
                Some(index_size) => {
                    debug_assert!(version == IlocVersion::One || version == IlocVersion::Two);
                    Some(iloc.read_u64(index_size.to_bits())?)
                }
            };

            // Per ISO 14496-12:2015 § 8.11.3.1:
            // "If the offset is not identified (the field has a length of zero), then the
            //  beginning of the source (offset 0) is implied"
            // This behavior will follow from BitReader::read_u64(0) -> 0.
            let extent_offset = iloc.read_u64(offset_size.to_bits())?;
            let extent_length = iloc.read_u64(length_size.to_bits())?;

            // "If the length is not specified, or specified as zero, then the entire length of
            //  the source is implied" (ibid)
            let start = base_offset
                .checked_add(extent_offset)
                .ok_or(Error::InvalidData("offset calculation overflow"))?;
            let extent_range = if extent_length == 0 {
                ExtentRange::ToEnd(RangeFrom { start })
            } else {
                let end = start
                    .checked_add(extent_length)
                    .ok_or(Error::InvalidData("end calculation overflow"))?;
                ExtentRange::WithLength(Range { start, end })
            };

            extents.push(ItemLocationBoxExtent { extent_range })?;
        }

        items.push(ItemLocationBoxItem {
            item_id,
            construction_method,
            extents,
        })?;
    }

    if iloc.remaining() == 0 {
        Ok(items)
    } else {
        Err(Error::InvalidData("invalid iloc size"))
    }
}

/// Parse an ftyp box.
/// See ISO 14496-12:2015 § 4.3
fn read_ftyp<T: Read>(src: &mut BMFFBox<'_, T>) -> Result<FileTypeBox> {
    let major = be_u32(src)?;
    let minor = be_u32(src)?;
    let bytes_left = src.bytes_left();
    if bytes_left % 4 != 0 {
        return Err(Error::InvalidData("invalid ftyp size"));
    }
    // Is a brand_count of zero valid?
    let brand_count = bytes_left / 4;
    let mut brands = TryVec::with_capacity(brand_count.try_into()?)?;
    for _ in 0..brand_count {
        brands.push(be_u32(src)?.into())?;
    }
    Ok(FileTypeBox {
        major_brand: From::from(major),
        minor_version: minor,
        compatible_brands: brands,
    })
}

/// Skip a number of bytes that we don't care to parse.
fn skip<T: Read>(src: &mut T, bytes: u64) -> Result<()> {
    std::io::copy(&mut src.take(bytes), &mut std::io::sink())?;
    Ok(())
}

fn be_u16<T: ReadBytesExt>(src: &mut T) -> Result<u16> {
    src.read_u16::<byteorder::BigEndian>().map_err(From::from)
}

fn be_u32<T: ReadBytesExt>(src: &mut T) -> Result<u32> {
    src.read_u32::<byteorder::BigEndian>().map_err(From::from)
}

fn be_u64<T: ReadBytesExt>(src: &mut T) -> Result<u64> {
    src.read_u64::<byteorder::BigEndian>().map_err(From::from)
}
