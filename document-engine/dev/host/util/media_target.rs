//! The raster decode limits the PDF arm bounds a decoded image by.

pub(crate) const RASTER_DECODE_MAX_ALLOC_BYTES: u64 = 256 * 1024 * 1024;

pub(crate) const RASTER_DECODE_MAX_DIMENSION_PX: u32 = 16384;
