#![allow(unused)]
#![allow(bad_style)]

use crate::{Error, Result};

use bitreader::BitReader;
use std::num::{NonZeroU32, NonZeroU8};

#[derive(Debug, Clone)]
struct Header {
    obu_size: usize,
    is_sequence_header: bool,
}

fn get_byte(data: &mut &[u8]) -> Result<u8> {
    let (&b, rest) = (*data).split_first().ok_or(Error::UnexpectedEOF)?;
    *data = rest;
    Ok(b)
}

const INTRA_FRAME: usize = 0;
const LAST_FRAME: usize = 1;
const LAST2_FRAME: usize = 2;
const LAST3_FRAME: usize = 3;
const GOLDEN_FRAME: usize = 4;
const BWDREF_FRAME: usize = 5;
const ALTREF2_FRAME: usize = 6;
const ALTREF_FRAME: usize = 7;

pub fn parse_obu(mut data: &[u8]) -> Result<SequenceHeaderObu> {
    while !data.is_empty() {
        let h = obu_header(&mut data)?;
        let mut remaining_data = data.get(..h.obu_size).ok_or(Error::UnexpectedEOF)?;
        data = &data[h.obu_size..];

        if h.is_sequence_header {
            return SequenceHeaderObu::read(remaining_data);
        }
    }
    Err(Error::UnexpectedEOF)
}

impl SequenceHeaderObu {
    fn read(data: &[u8]) -> Result<Self> {
        let mut b = BitReader::new(data);
        let mut enable_superres = false;
        let mut enable_cdef = false;
        let mut enable_restoration = false;

        let seq_profile = b.read_u8(3)?;
        if seq_profile > 2 {
            return Err(Error::InvalidData("seq_profile"));
        }
        let still_picture = b.read_bool()?;
        let reduced_still_picture_header = b.read_bool()?;

        let decoder_model_info_present_flag = false;
        if reduced_still_picture_header {
            let timing_info_present_flag = 0;
            let initial_display_delay_present_flag = 0;
            let operating_points_cnt_minus_1 = 0;
            let operating_point_idc = 0; // [ 0 ]
            let seq_level_idx = b.read_u8(5)?;
            let seq_tier = 0; // [ 0 ]
            let decoder_model_present_for_this_op = 0; // [ 0 ]
            let initial_display_delay_present_for_this_op = 0; // [ 0 ]
        } else {
            let timing_info_present_flag = b.read_bool()?;
            if timing_info_present_flag {
                return Err(Error::Unsupported("timing_info_present_flag"));
            }
            let initial_display_delay_present_flag = b.read_bool()?;
            let operating_points_cnt = 1 + b.read_u8(5)?;

            for _ in 0..operating_points_cnt {
                let operating_point_idc = b.read_u16(12)?;
                let seq_level_idx = b.read_u8(5)?;
                let seq_tier = if seq_level_idx > 7 { b.read_bool()? } else { false };
                let decoder_model_present_for_this_op = if decoder_model_info_present_flag {
                    b.read_bool()?;
                    return Err(Error::Unsupported("decoder_model_info_present_flag"));
                } else {
                    false
                };
                if initial_display_delay_present_flag {
                    let initial_display_delay_present_for_this_op = b.read_bool()?;
                    if initial_display_delay_present_for_this_op {
                        let initial_display_delay = 1 + b.read_u8(4)?;
                    }
                }
            }
            // let operating_point = choose_operating_point();
            // let OperatingPointIdc = operating_point_idc[ operating_point ];
        }
        let frame_width_bits = 1 + b.read_u8(4)?;
        let frame_height_bits = 1 + b.read_u8(4)?;
        let frame_width_bits = NonZeroU8::new(frame_width_bits).ok_or(Error::InvalidData("overflow"))?;
        let frame_height_bits = NonZeroU8::new(frame_height_bits).ok_or(Error::InvalidData("overflow"))?;

        let max_frame_width = 1 + b.read_u32(frame_width_bits.get())?;
        let max_frame_height = 1 + b.read_u32(frame_height_bits.get())?;
        let max_frame_width = NonZeroU32::new(max_frame_width).ok_or(Error::InvalidData("overflow"))?;
        let max_frame_height = NonZeroU32::new(max_frame_height).ok_or(Error::InvalidData("overflow"))?;

        let frame_id_numbers_present_flag = if reduced_still_picture_header { false } else { b.read_bool()? };
        let delta_frame_id_length = if frame_id_numbers_present_flag { 2 + b.read_u8(4)? } else { 0 };
        let additional_frame_id_length = if frame_id_numbers_present_flag { 1 + b.read_u8(3)? } else { 0 };

        let use_128x128_superblock = b.read_bool()?;
        let enable_filter_intra = b.read_bool()?;
        let enable_intra_edge_filter = b.read_bool()?;

        let mut enable_interintra_compound = false;
        let mut enable_masked_compound = false;
        let mut enable_warped_motion = false;
        let mut enable_dual_filter = false;
        let mut enable_jnt_comp = false;
        let mut enable_ref_frame_mvs = false;
        let mut seq_force_screen_content_tools = SELECT_SCREEN_CONTENT_TOOLS;
        let mut seq_force_integer_mv = SELECT_INTEGER_MV;
        let mut order_hint_bits = 0;
        let mut enable_order_hint = false;

        if !reduced_still_picture_header {
            enable_interintra_compound = b.read_bool()?;
            enable_masked_compound = b.read_bool()?;
            enable_warped_motion = b.read_bool()?;
            enable_dual_filter = b.read_bool()?;
            enable_order_hint = b.read_bool()?;
            if enable_order_hint {
                enable_jnt_comp = b.read_bool()?;
                enable_ref_frame_mvs = b.read_bool()?;
            }
            let seq_choose_screen_content_tools = b.read_bool()?;
            if !seq_choose_screen_content_tools {
                seq_force_screen_content_tools = b.read_u8(1)?;
            }

            if seq_force_screen_content_tools > 0 {
                let seq_choose_integer_mv = b.read_bool()?;
                if !seq_choose_integer_mv {
                    seq_force_integer_mv = b.read_u8(1)?;
                }
            }
            if enable_order_hint {
                order_hint_bits = 1 + b.read_u8(3)?;
            }
        }
        let enable_superres = b.read_bool()?;
        let enable_cdef = b.read_bool()?;
        let enable_restoration = b.read_bool()?;
        let color = color_config(&mut b, seq_profile)?;
        let film_grain_params_present = b.read_bool()?;

        Ok(Self {
            color,
            seq_profile,
            still_picture,
            reduced_still_picture_header,
            max_frame_width,
            max_frame_height,
            enable_superres,
            enable_cdef,
            enable_restoration,
            frame_id_numbers_present_flag,
            delta_frame_id_length,
            additional_frame_id_length,
            film_grain_params_present,
            decoder_model_info_present_flag,
            seq_force_screen_content_tools,
            seq_force_integer_mv,
            order_hint_bits,
            enable_order_hint,
            use_128x128_superblock,
            enable_interintra_compound,
            enable_masked_compound,
            enable_warped_motion,
            enable_dual_filter,
            enable_jnt_comp,
            enable_ref_frame_mvs,
        })
    }
}

#[derive(Debug, Clone)]
pub(crate) struct SequenceHeaderObu {
    pub color: ColorConfig,

    pub seq_profile: u8,
    pub still_picture: bool,
    pub reduced_still_picture_header: bool,

    pub max_frame_width: NonZeroU32,
    pub max_frame_height: NonZeroU32,

    pub enable_superres: bool,
    pub enable_cdef: bool,
    pub enable_restoration: bool,

    pub frame_id_numbers_present_flag: bool,
    pub delta_frame_id_length: u8,
    pub additional_frame_id_length: u8,
    pub film_grain_params_present: bool,
    pub decoder_model_info_present_flag: bool,
    pub seq_force_screen_content_tools: u8,
    pub seq_force_integer_mv: u8,
    pub order_hint_bits: u8,
    pub enable_order_hint: bool,
    pub use_128x128_superblock: bool,

    pub enable_interintra_compound: bool,
    pub enable_masked_compound: bool,
    pub enable_warped_motion: bool,
    pub enable_dual_filter: bool,
    pub enable_jnt_comp: bool,
    pub enable_ref_frame_mvs: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct ColorConfig {
    pub subsampling_x: u8,
    pub subsampling_y: u8,
    pub chroma_sample_position: u8,
    pub separate_uv_delta_q: bool,
    pub color_range: u8,
    pub bit_depth: u8,
    pub mono_chrome: bool,

    pub color_primaries: u8,
    pub transfer_characteristics: u8,
    pub matrix_coefficients: u8,
}

fn color_config(b: &mut BitReader, seq_profile: u8) -> Result<ColorConfig> {
    let high_bitdepth = b.read_bool()?;
    let bit_depth = if seq_profile == 2 && high_bitdepth {
        let twelve_bit = b.read_bool()?;
        if twelve_bit {
            12
        } else {
            10
        }
    } else { // if seq_profile <= 2
        if high_bitdepth {
            10
        } else {
            8
        }
    };

    let mono_chrome = if seq_profile == 1 { false } else { b.read_bool()? };

    let num_planes = if mono_chrome { 1 } else { 3 };
    let color_description_present_flag = b.read_bool()?;
    let mut color_primaries = 2;
    let mut transfer_characteristics = 2;
    let mut matrix_coefficients = 2;
    if color_description_present_flag {
        color_primaries = b.read_u8(8)?;
        transfer_characteristics = b.read_u8(8)?;
        matrix_coefficients = b.read_u8(8)?;
    }

    let subsampling_x;
    let subsampling_y;
    let chroma_sample_position;
    let separate_uv_delta_q;
    let color_range;
    if mono_chrome {
        color_range = b.read_u8(1)?;
        subsampling_x = 0;
        subsampling_y = 0;
        chroma_sample_position = 0;
        separate_uv_delta_q = false;
    } else if color_primaries == 1 //Bt709
        && transfer_characteristics == 13  // Srgb
        && matrix_coefficients == 0
    {
        color_range = 1;
        subsampling_x = 0;
        subsampling_y = 0;
        chroma_sample_position = 0;
        separate_uv_delta_q = false;
    } else {
        color_range = b.read_u8(1)?;
        if seq_profile == 0 {
            subsampling_x = 1;
            subsampling_y = 1;
        } else if seq_profile == 1 {
            subsampling_x = 0;
            subsampling_y = 0;
        } else if bit_depth == 12 {
            subsampling_x = b.read_u8(1)?;
            if subsampling_x != 0 {
                subsampling_y = b.read_u8(1)?;
            } else {
                subsampling_y = 0;
            }
        } else {
            subsampling_x = 1;
            subsampling_y = 0;
        }
        debug_assert!(!mono_chrome);
        chroma_sample_position = if subsampling_x != 0 && subsampling_y != 0 { b.read_u8(2)? } else { 0 };
        separate_uv_delta_q = b.read_bool()?;
    }

    Ok(ColorConfig {
        subsampling_x,
        subsampling_y,
        chroma_sample_position,
        separate_uv_delta_q,
        color_range,
        bit_depth,
        mono_chrome,

        color_primaries,
        transfer_characteristics,
        matrix_coefficients,
    })
}

fn obu_header(data: &mut &[u8]) -> Result<Header> {
    let mut b = get_byte(data)?;
    if 0 != b & 0b1000_0000 {
        return Err(Error::InvalidData("not obu"));
    }

    let is_sequence_header = 1 == (b >> 3);
    let obu_extension_flag = 0 != (b & 0b100);
    let obu_has_size_field = 0 != (b & 0b010);

    if obu_extension_flag {
        // obu_extension_header
        let mut b = get_byte(data)?;
    }

    let obu_size = if obu_has_size_field {
        leb128::read::unsigned(data)
            .map_err(|_| Error::InvalidData("leb"))?
            .try_into()
            .map_err(|_| Error::UnexpectedEOF)?
    } else {
        data.len()
    };

    Ok(Header { obu_size, is_sequence_header })
}

const REFS_PER_FRAME: usize = 7; //   Number of reference frames that can be used for inter prediction
const TOTAL_REFS_PER_FRAME: usize = 8; //   Number of reference frame types (including intra type)
const BLOCK_SIZE_GROUPS: usize = 4; //   Number of contexts when decoding y_mode
const BLOCK_SIZES: usize = 22; //  Number of different block sizes used
const BLOCK_INVALID: usize = 22; //  Sentinel value to mark partition choices that are not allowed
const MAX_SB_SIZE: usize = 128; //     Maximum size of a superblock in luma samples
const MI_SIZE: usize = 4; //   Smallest size of a mode info block in luma samples
const MI_SIZE_LOG2: usize = 2; //   Base 2 logarithm of smallest size of a mode info block
const MAX_TILE_WIDTH: usize = 4096; //    Maximum width of a tile in units of luma samples
const MAX_TILE_AREA: usize = 4096; // * 2304     Maximum area of a tile in units of luma samples
const MAX_TILE_ROWS: usize = 64; //  Maximum number of tile rows
const MAX_TILE_COLS: usize = 64; //  Maximum number of tile columns
const INTRABC_DELAY_PIXELS: usize = 256; //     Number of horizontal luma samples before intra block copy can be used
const INTRABC_DELAY_SB64: usize = 4; //   Number of 64 by 64 blocks before intra block copy can be used
const NUM_REF_FRAMES: usize = 8; //   Number of frames that can be stored for future reference
const REF_CONTEXTS: usize = 3; //   Number of contexts for single_ref, comp_ref, comp_bwdref, uni_comp_ref, uni_comp_ref_p1 and uni_comp_ref_p2
const MAX_SEGMENTS: usize = 8; //   Number of segments allowed in segmentation map
const SEGMENT_ID_CONTEXTS: usize = 3; //   Number of contexts for segment_id
const SEG_LVL_ALT_Q: usize = 0; //   Index for quantizer segment feature
const SEG_LVL_ALT_LF_Y_V: usize = 1; //   Index for vertical luma loop filter segment feature
const SEG_LVL_REF_FRAME: usize = 5; //   Index for reference frame segment feature
const SEG_LVL_SKIP: usize = 6; //   Index for skip segment feature
const SEG_LVL_GLOBALMV: usize = 7; //   Index for global mv feature
const SEG_LVL_MAX: usize = 8; //   Number of segment features
const PLANE_TYPES: usize = 2; //   Number of different plane types (luma or chroma)
const TX_SIZE_CONTEXTS: usize = 3; //   Number of contexts for transform size
const INTERP_FILTERS: usize = 3; //   Number of values for interp_filter
const INTERP_FILTER_CONTEXTS: usize = 16; //  Number of contexts for interp_filter
const SKIP_MODE_CONTEXTS: usize = 3; //   Number of contexts for decoding skip_mode
const SKIP_CONTEXTS: usize = 3; //   Number of contexts for decoding skip
const PARTITION_CONTEXTS: usize = 4; //   Number of contexts when decoding partition
const TX_SIZES: usize = 5; //   Number of square transform sizes
const TX_SIZES_ALL: usize = 19; //  Number of transform sizes (including non-square sizes)
const TX_MODES: usize = 3; //   Number of values for tx_mode
const DCT_DCT: usize = 0; //   Inverse transform rows with DCT and columns with DCT
const ADST_DCT: usize = 1; //   Inverse transform rows with DCT and columns with ADST
const DCT_ADST: usize = 2; //   Inverse transform rows with ADST and columns with DCT
const ADST_ADST: usize = 3; //   Inverse transform rows with ADST and columns with ADST
const FLIPADST_DCT: usize = 4; //   Inverse transform rows with DCT and columns with FLIPADST
const DCT_FLIPADST: usize = 5; //   Inverse transform rows with FLIPADST and columns with DCT
const FLIPADST_FLIPADST: usize = 6; //   Inverse transform rows with FLIPADST and columns with FLIPADST
const ADST_FLIPADST: usize = 7; //   Inverse transform rows with FLIPADST and columns with ADST
const FLIPADST_ADST: usize = 8; //   Inverse transform rows with ADST and columns with FLIPADST
const IDTX: usize = 9; //   Inverse transform rows with identity and columns with identity
const V_DCT: usize = 10; //  Inverse transform rows with identity and columns with DCT
const H_DCT: usize = 11; //  Inverse transform rows with DCT and columns with identity
const V_ADST: usize = 12; //  Inverse transform rows with identity and columns with ADST
const H_ADST: usize = 13; //  Inverse transform rows with ADST and columns with identity
const V_FLIPADST: usize = 14; //  Inverse transform rows with identity and columns with FLIPADST
const H_FLIPADST: usize = 15; //  Inverse transform rows with FLIPADST and columns with identity
const TX_TYPES: usize = 16; //  Number of inverse transform types
const MB_MODE_COUNT: usize = 17; //  Number of values for YMode
const INTRA_MODES: usize = 13; //  Number of values for y_mode
const UV_INTRA_MODES_CFL_NOT_ALLOWED: usize = 13; //  Number of values for uv_mode when chroma from luma is not allowed
const UV_INTRA_MODES_CFL_ALLOWED: usize = 14; //  Number of values for uv_mode when chroma from luma is allowed
const COMPOUND_MODES: usize = 8; //   Number of values for compound_mode
const COMPOUND_MODE_CONTEXTS: usize = 8; //   Number of contexts for compound_mode
const COMP_NEWMV_CTXS: usize = 5; //   Number of new mv values used when constructing context for compound_mode
const NEW_MV_CONTEXTS: usize = 6; //   Number of contexts for new_mv
const ZERO_MV_CONTEXTS: usize = 2; //   Number of contexts for zero_mv
const REF_MV_CONTEXTS: usize = 6; //   Number of contexts for ref_mv
const DRL_MODE_CONTEXTS: usize = 3; //   Number of contexts for drl_mode
const MV_CONTEXTS: usize = 2; //   Number of contexts for decoding motion vectors including one for intra block copy
const MV_INTRABC_CONTEXT: usize = 1; //   Motion vector context used for intra block copy
const MV_JOINTS: usize = 4; //   Number of values for mv_joint
const MV_CLASSES: usize = 11; //  Number of values for mv_class
const CLASS0_SIZE: usize = 2; //   Number of values for mv_class0_bit
const MV_OFFSET_BITS: usize = 10; //  Maximum number of bits for decoding motion vectors
const MAX_LOOP_FILTER: usize = 63; //  Maximum value used for loop filtering
const REF_SCALE_SHIFT: usize = 14; //  Number of bits of precision when scaling reference frames
const SUBPEL_BITS: usize = 4; //   Number of bits of precision when choosing an inter prediction filter kernel
const SUBPEL_MASK: usize = 15; //  ( 1 << SUBPEL_BITS ) - 1
const SCALE_SUBPEL_BITS: usize = 10; //  Number of bits of precision when computing inter prediction locations
const MV_BORDER: usize = 128; //     Value used when clipping motion vectors
const PALETTE_COLOR_CONTEXTS: usize = 5; //   Number of values for color contexts
const PALETTE_MAX_COLOR_CONTEXT_HASH: usize = 8; //   Number of mappings between color context hash and color context
const PALETTE_BLOCK_SIZE_CONTEXTS: usize = 7; //   Number of values for palette block size
const PALETTE_Y_MODE_CONTEXTS: usize = 3; //   Number of values for palette Y plane mode contexts
const PALETTE_UV_MODE_CONTEXTS: usize = 2; //   Number of values for palette U and V plane mode contexts
const PALETTE_SIZES: usize = 7; //   Number of values for palette_size
const PALETTE_COLORS: usize = 8; //   Number of values for palette_color
const PALETTE_NUM_NEIGHBORS: usize = 3; //   Number of neighbors considered within palette computation
const DELTA_Q_SMALL: usize = 3; //   Value indicating alternative encoding of quantizer index delta values
const DELTA_LF_SMALL: usize = 3; //   Value indicating alternative encoding of loop filter delta values
const QM_TOTAL_SIZE: usize = 3344; //    Number of values in the quantizer matrix
const MAX_ANGLE_DELTA: usize = 3; //   Maximum magnitude of AngleDeltaY and AngleDeltaUV
const DIRECTIONAL_MODES: usize = 8; //   Number of directional intra modes
const ANGLE_STEP: usize = 3; //   Number of degrees of step per unit increase in AngleDeltaY or AngleDeltaUV.
const TX_SET_TYPES_INTRA: usize = 3; //   Number of intra transform set types
const TX_SET_TYPES_INTER: usize = 4; //   Number of inter transform set types
const WARPEDMODEL_PREC_BITS: usize = 16; //  Internal precision of warped motion models
const IDENTITY: usize = 0; //   Warp model is just an identity transform
const TRANSLATION: usize = 1; //   Warp model is a pure translation
const ROTZOOM: usize = 2; //   Warp model is a rotation + symmetric zoom + translation
const AFFINE: usize = 3; //   Warp model is a general affine transform
const GM_ABS_TRANS_BITS: usize = 12; //  Number of bits encoded for translational components of global motion models, if part of a ROTZOOM or AFFINE model
const GM_ABS_TRANS_ONLY_BITS: usize = 9; //   Number of bits encoded for translational components of global motion models, if part of a TRANSLATION model
const GM_ABS_ALPHA_BITS: usize = 12; //  Number of bits encoded for non-translational components of global motion models
const DIV_LUT_PREC_BITS: usize = 14; //  Number of fractional bits of entries in divisor lookup table
const DIV_LUT_BITS: usize = 8; //   Number of fractional bits for lookup in divisor lookup table
const DIV_LUT_NUM: usize = 257; //     Number of entries in divisor lookup table
const MOTION_MODES: usize = 3; //   Number of values for motion modes
const SIMPLE: usize = 0; //   Use translation or global motion compensation
const OBMC: usize = 1; //   Use overlapped block motion compensation
const LOCALWARP: usize = 2; //   Use local warp motion compensation
const LEAST_SQUARES_SAMPLES_MAX: usize = 8; //   Largest number of samples used when computing a local warp
const LS_MV_MAX: usize = 256; //     Largest motion vector difference to include in local warp computation
const WARPEDMODEL_TRANS_CLAMP: usize = 1; //<<23   Clamping value used for translation components of warp
const WARPEDMODEL_NONDIAGAFFINE_CLAMP: usize = 1; //<<13   Clamping value used for matrix components of warp
const WARPEDPIXEL_PREC_SHIFTS: usize = 1; //<<6    Number of phases used in warped filtering
const WARPEDDIFF_PREC_BITS: usize = 10; //  Number of extra bits of precision in warped filtering
const GM_ALPHA_PREC_BITS: usize = 15; //  Number of fractional bits for sending non-translational warp model coefficients
const GM_TRANS_PREC_BITS: usize = 6; //   Number of fractional bits for sending translational warp model coefficients
const GM_TRANS_ONLY_PREC_BITS: usize = 3; //   Number of fractional bits used for pure translational warps
const INTERINTRA_MODES: usize = 4; //   Number of inter intra modes
const MASK_MASTER_SIZE: usize = 64; //  Size of MasterMask array
const SEGMENT_ID_PREDICTED_CONTEXTS: usize = 3; //   Number of contexts for segment_id_predicted
const IS_INTER_CONTEXTS: usize = 4; //   Number of contexts for is_inter
const FWD_REFS: usize = 4; //   Number of syntax elements for forward reference frames
const BWD_REFS: usize = 3; //   Number of syntax elements for backward reference frames
const SINGLE_REFS: usize = 7; //   Number of syntax elements for single reference frames
const UNIDIR_COMP_REFS: usize = 4; //   Number of syntax elements for unidirectional compound reference frames
const COMPOUND_TYPES: usize = 2; //   Number of values for compound_type
const CFL_JOINT_SIGNS: usize = 8; //   Number of values for cfl_alpha_signs
const CFL_ALPHABET_SIZE: usize = 16; //  Number of values for cfl_alpha_u and cfl_alpha_v
const COMP_INTER_CONTEXTS: usize = 5; //   Number of contexts for comp_mode
const COMP_REF_TYPE_CONTEXTS: usize = 5; //   Number of contexts for comp_ref_type
const CFL_ALPHA_CONTEXTS: usize = 6; //   Number of contexts for cfl_alpha_u and cfl_alpha_v
const INTRA_MODE_CONTEXTS: usize = 5; //   Number of each of left and above contexts for intra_frame_y_mode
const COMP_GROUP_IDX_CONTEXTS: usize = 6; //   Number of contexts for comp_group_idx
const COMPOUND_IDX_CONTEXTS: usize = 6; //   Number of contexts for compound_idx
const INTRA_EDGE_KERNELS: usize = 3; //   Number of filter kernels for the intra edge filter
const INTRA_EDGE_TAPS: usize = 5; //   Number of kernel taps for the intra edge filter
const FRAME_LF_COUNT: usize = 4; //   Number of loop filter strength values
const MAX_VARTX_DEPTH: usize = 2; //   Maximum depth for variable transform trees
const TXFM_PARTITION_CONTEXTS: usize = 21; //  Number of contexts for txfm_split
const REF_CAT_LEVEL: usize = 640; //     Bonus weight for close motion vectors
const MAX_REF_MV_STACK_SIZE: usize = 8; //   Maximum number of motion vectors in the stack
const MFMV_STACK_SIZE: usize = 3; //   Stack size for motion field motion vectors
const MAX_TX_DEPTH: usize = 2; //   Maximum times the transform can be split
const WEDGE_TYPES: usize = 16; //  Number of directions for the wedge mask process
const FILTER_BITS: usize = 7; //   Number of bits used in Wiener filter coefficients
const WIENER_COEFFS: usize = 3; //   Number of Wiener filter coefficients to read
const SGRPROJ_PARAMS_BITS: usize = 4; //   Number of bits needed to specify self guided filter set
const SGRPROJ_PRJ_SUBEXP_K: usize = 4; //   Controls how self guided deltas are read
const SGRPROJ_PRJ_BITS: usize = 7; //   Precision bits during self guided restoration
const SGRPROJ_RST_BITS: usize = 4; //   Restoration precision bits generated higher than source before projection
const SGRPROJ_MTABLE_BITS: usize = 20; //  Precision of mtable division table
const SGRPROJ_RECIP_BITS: usize = 12; //  Precision of division by n table
const SGRPROJ_SGR_BITS: usize = 8; //   Internal precision bits for core selfguided_restoration
const EC_PROB_SHIFT: usize = 6; //   Number of bits to reduce CDF precision during arithmetic coding
const EC_MIN_PROB: usize = 4; //   Minimum probability assigned to each symbol during arithmetic coding
const SELECT_SCREEN_CONTENT_TOOLS: u8 = 2; //   Value that indicates the allow_screen_content_tools syntax element is coded
const SELECT_INTEGER_MV: u8 = 2; //   Value that indicates the force_integer_mv syntax element is coded
const RESTORATION_TILESIZE_MAX: usize = 256; //     Maximum size of a loop restoration tile
const MAX_FRAME_DISTANCE: usize = 31; //  Maximum distance when computing weighted prediction
const MAX_OFFSET_WIDTH: usize = 8; //   Maximum horizontal offset of a projected motion vector
const MAX_OFFSET_HEIGHT: usize = 0; //   Maximum vertical offset of a projected motion vector
const WARP_PARAM_REDUCE_BITS: usize = 6; //   Rounding bitwidth for the parameters to the shear process
const NUM_BASE_LEVELS: usize = 2; //   Number of quantizer base levels
const COEFF_BASE_RANGE: usize = 12; //  The quantizer range above NUM_BASE_LEVELS above which the Exp-Golomb coding process is activated
const BR_CDF_SIZE: usize = 4; //   Number of values for coeff_br
const SIG_COEF_CONTEXTS_EOB: usize = 4; //   Number of contexts for coeff_base_eob
const SIG_COEF_CONTEXTS_2D: usize = 26; //  Context offset for coeff_base for horizontal-only or vertical-only transforms.
const SIG_COEF_CONTEXTS: usize = 42; //  Number of contexts for coeff_base
const SIG_REF_DIFF_OFFSET_NUM: usize = 5; //   Maximum number of context samples to be used in determining the context index for coeff_base and coeff_base_eob.
const SUPERRES_NUM: usize = 8; //   Numerator for upscaling ratio
const SUPERRES_DENOM_MIN: usize = 9; //   Smallest denominator for upscaling ratio
const SUPERRES_DENOM_BITS: usize = 3; //   Number of bits sent to specify denominator of upscaling ratio
const SUPERRES_FILTER_BITS: usize = 6; //   Number of bits of fractional precision for upscaling filter selection
const SUPERRES_FILTER_SHIFTS: usize = 1; // << SUPERRES_FILTER_BITS   Number of phases of upscaling filters
const SUPERRES_FILTER_TAPS: usize = 8; //   Number of taps of upscaling filters
const SUPERRES_FILTER_OFFSET: usize = 3; //   Sample offset for upscaling filters
const SUPERRES_SCALE_BITS: usize = 14; //  Number of fractional bits for computing position in upscaling
const SUPERRES_SCALE_MASK: usize = (1 << 14) - 1; // Mask for computing position in upscaling
const SUPERRES_EXTRA_BITS: usize = 8; //   Difference in precision between SUPERRES_SCALE_BITS and SUPERRES_FILTER_BITS
const TXB_SKIP_CONTEXTS: usize = 13; //  Number of contexts for all_zero
const EOB_COEF_CONTEXTS: usize = 9; //   Number of contexts for eob_extra
const DC_SIGN_CONTEXTS: usize = 3; //   Number of contexts for dc_sign
const LEVEL_CONTEXTS: usize = 21; //  Number of contexts for coeff_br
const TX_CLASS_2D: usize = 0; //   Transform class for transform types performing non-identity transforms in both directions
const TX_CLASS_HORIZ: usize = 1; //   Transform class for transforms performing only a horizontal non-identity transform
const TX_CLASS_VERT: usize = 2; //   Transform class for transforms performing only a vertical non-identity transform
const REFMVS_LIMIT: usize = (1 << 12) - 1; //      Largest reference MV component that can be saved
const INTRA_FILTER_SCALE_BITS: usize = 4; //   Scaling shift for intra filtering process
const INTRA_FILTER_MODES: usize = 5; //   Number of types of intra filtering
const COEFF_CDF_Q_CTXS: usize = 4; //   Number of selectable context types for the coeff( ) syntax structure
const PRIMARY_REF_NONE: usize = 7; //   Value of primary_ref_frame indicating that there is no primary reference frame
const BUFFER_POOL_MAX_SIZE: usize = 10; //  Number of frames in buffer pool
