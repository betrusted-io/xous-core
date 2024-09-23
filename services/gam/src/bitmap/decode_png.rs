/*
 * This incomplete PNG decoder presents as an Iterator reading u8 bytes directly from a Reader.
 *
 * Aproaching a png decoder as an Iterator requires significantly less Heap than
 * the traditional batch aproach.
 *
 * On Precursor, this enables PNG bytes to be intercepted at the network interface
 * and processed into the much smaller gam::Bitmap format without ever having to
 * store the original PNG file.
 *
 *
 * The png image format includes a signature (8  bytes) followed by chunks
 * - Each chunk includes the chunk begins with its length (4 bytes) and name (4 bytes) and ends with a
 *   checksum (4 bytes)
 * - The IHDR chunk must be first and contains width, height, bit_depth, color_type, compression_method,
 *   filter_method, and an interlace flag.
 * - The Palette chunck is currently unsupported
 * - Consecutive IDAT chunks hold the image data in bytes.
 * - The IEND chunk must be last.
 * - Other chunk types are ignored in this decoder.
 * - The bytes in the IDAT chunks must be concatenated and uncompressed (zlib)
 * - The uncompressed bytes are arranged in scanlines, left-right, top-bottom.
 * - The first byte in each scan-line representes a filter_type which is used to unfilter the remaining
 *   bytes in the scan-line. (filters is used to improve compression)
 * - The uncompressed, unfiltered bytes now represent pixels in the specified color_type (eg RGB, RGBa etc)
 *
 * This development is gratefully based on PNG Pong - Copyright © 2019-2021 Jeron Aldaron Lau
 *
 * author: nworbnhoj
 */

use std::convert::TryInto;
use std::io::{Error, ErrorKind::InvalidData, Read, Result};

use miniz_oxide::{DataFormat, MZFlush, MZStatus, inflate::stream::InflateState};
//use pix::rgb::SRgb8;

mod color_type;
use color_type::*;

// Magic bytes to start a PNG file.
pub(super) const PNG_SIGNATURE: [u8; 8] = [137, 80, 78, 71, 13, 10, 26, 10];

// Chunk Identifiers
pub(super) const IMAGE_HEADER: [u8; 4] = *b"IHDR";
pub(super) const IMAGE_DATA: [u8; 4] = *b"IDAT";
pub(super) const IMAGE_END: [u8; 4] = *b"IEND";
pub(super) const PALETTE: [u8; 4] = *b"PLTE";

pub(super) const MAX_CHUNK_SIZE: usize = 1 << 31; // 2³¹

// These may be adjusted to tune the heap requirement.
// Probably better for INFLATED_BUFFER = 2-3x IDAT_BUFFER
// TODO benchmarking speed vs heap
const IDAT_BUFFER_LENGTH: usize = 256;
const INFLATED_BUFFER_LENGTH: usize = 1024;

pub struct DecodePng<R: Read> {
    reader: R,
    // IHDR chunk fields
    width: u32,
    height: u32,
    bit_depth: u8,
    color_type: u8,
    compression_method: u8,
    filter_method: u8,
    interlace: bool,
    // PLTE chunk fields
    palette: Option<Vec<(u8, u8, u8)>>,
    //palette: Option<Vec<SRgb8>>,
    // fields to manage the filter
    bytes_per_pixel: usize,
    bytes_per_line: usize,
    byte_index: usize,
    filter_type: u8,
    prior_line: Vec<u8>,
    line_index: usize,
    prior_px: Vec<u8>,
    // fields to buffer bytes read from the idat chunk
    idat_remaining: u32,
    idat_buffer: Vec<u8>,
    // state of the miniz oxide inflation
    inflate_state: Box<InflateState>,
    // fields to buffer inflated bytes waiting to be unfiltered
    inflated: [u8; INFLATED_BUFFER_LENGTH],
    inflated_length: usize,
    inflated_index: usize,
}

impl<R: Read> Iterator for DecodePng<R> {
    type Item = u8;

    fn next(&mut self) -> Option<Self::Item> { self.unfilter() }
}

impl<R: Read> DecodePng<R> {
    pub fn new(reader: R) -> Result<DecodePng<R>> {
        let mut png = Self {
            reader,
            width: 0,
            height: 0,
            bit_depth: 0,
            color_type: 0,
            compression_method: 0,
            filter_method: 0,
            interlace: false,
            palette: None,
            bytes_per_pixel: 0,
            bytes_per_line: 0,
            byte_index: 0,
            filter_type: 0,
            prior_line: vec![0u8, 0],
            line_index: 0,
            prior_px: vec![0u8, 0],
            idat_remaining: 0,
            idat_buffer: Vec::with_capacity(IDAT_BUFFER_LENGTH),
            inflate_state: InflateState::new_boxed(DataFormat::Zlib),
            inflated: [0u8; INFLATED_BUFFER_LENGTH],
            inflated_length: 0,
            inflated_index: INFLATED_BUFFER_LENGTH,
        };
        png.check_signature()?;
        png.parse_header()?;
        if png.color_type == 3 {
            png.parse_palette()?;
        }
        png.idat_remaining = png.next_idat(false)?;
        png.fill_idat_buffer(IDAT_BUFFER_LENGTH)?;
        png.set_filter_type();
        log::info!(
            "DecodePng ready: size({},{}) bit-depth={} color_type={} bytes_per_line={}",
            png.width,
            png.height,
            png.bit_depth,
            png.color_type,
            png.bytes_per_line,
        );
        Ok(png)
    }

    pub fn width(&self) -> u32 { self.width }

    pub fn height(&self) -> u32 { self.height }

    pub fn color_type(&self) -> u8 { self.color_type }

    pub fn bit_depth(&self) -> u8 { self.bit_depth }

    /// Prepare a chunk for reading, capturing name & length.
    pub(crate) fn prepare_chunk(&mut self) -> Result<([u8; 4], u32)> {
        let length = match self.u8() {
            Ok(first) => u32::from_be_bytes([first, self.u8()?, self.u8()?, self.u8()?]),
            Err(err) => {
                log::warn!("failed to decode png chunk: {:?}", err);
                0
            }
        };
        let name = [self.u8()?, self.u8()?, self.u8()?, self.u8()?];
        if length > MAX_CHUNK_SIZE as u32 {
            log::warn!("failed to decode png chunk: oversize");
        }
        log::trace!("found png chunk name={:?} with length={}", String::from_utf8_lossy(&name), length);
        Ok((name, length))
    }

    fn check_signature(&mut self) -> Result<()> {
        // Read first 8 bytes (PNG Signature)
        let mut buf = [0u8; 8];
        self.reader.read_exact(&mut buf)?;
        match buf {
            PNG_SIGNATURE => {
                log::trace!("png signature matched");
                Ok(())
            }
            _ => Err(Error::new(InvalidData, "invalid png signature")),
        }
    }

    fn parse_header(&mut self) -> Result<()> {
        let color_type: ColorType;
        match self.prepare_chunk()? {
            (IMAGE_HEADER, _) => {
                log::trace!("located png header chunk");
                self.width = self.u32()?;
                self.height = self.u32()?;
                self.bit_depth = self.u8()?;
                self.color_type = self.u8()?;
                self.compression_method = self.u8()?;
                self.filter_method = self.u8()?;
                self.interlace = match self.u8()? {
                    0 => false,
                    1 => true,
                    _ => return Err(Error::new(InvalidData, "invalid interlace")),
                };
                self.ignore_crc()?;

                color_type = match self.color_type {
                    0 => ColorType::Grey,
                    2 => ColorType::Rgb,
                    3 => ColorType::Palette,
                    4 => ColorType::GreyAlpha,
                    6 => ColorType::Rgba,
                    _ => return Err(Error::new(InvalidData, "invalid color-type")),
                };
                self.bytes_per_pixel = (color_type.bpp(self.bit_depth) / 8).try_into().unwrap();
                self.bytes_per_line = 1 + self.bytes_per_pixel * self.width as usize;
                // png filter specification calls for initial prior_line = [0u8]
                // see: https://www.w3.org/TR/PNG/#9Filters
                // but [0u8] results in distortion???????
                // TODO fix this properly (probably a byte overflow or bad cast)
                self.prior_line = vec![125u8; self.bytes_per_line];
                self.prior_px = vec![0u8; self.bytes_per_pixel];
            }
            (_, _) => return Err(Error::new(InvalidData, "header chunk not first")),
        };

        if self.width == 0 || self.height == 0 {
            return Err(Error::new(InvalidData, "invalid image dimensions"));
        } else if self.bit_depth == 0 || self.bit_depth > 16 {
            return Err(Error::new(InvalidData, "invalid bit depth"));
        } else if self.compression_method != 0 {
            return Err(Error::new(InvalidData, "invalid compression method"));
        } else if self.filter_method != 0 {
            return Err(Error::new(InvalidData, "invalid filter method"));
        } else if self.interlace {
            return Err(Error::new(InvalidData, "interlace is not supported"));
        } else if self.bytes_per_pixel < 1 {
            return Err(Error::new(InvalidData, "less than 8 bits per pixel is not supported"));
        } else if self.color_type == 3 {
            return Err(Error::new(InvalidData, "color_type = 3 palette is not supported"));
        } else {
            color_type.check_png_color_validity(self.bit_depth)?;
        };
        Ok(())
    }

    fn parse_palette(&mut self) -> Result<()> {
        while self.palette.is_none() {
            self.palette = match self.prepare_chunk()? {
                (PALETTE, length) => {
                    log::trace!("located png palette chunk");
                    let mut palette = Vec::new();
                    for _ in 0..(length / 3) {
                        let red = self.u8()?;
                        let green = self.u8()?;
                        let blue = self.u8()?;
                        palette.push((red, green, blue));
                        //palette.push(SRgb8::new(red, green, blue));
                    }
                    self.ignore_crc()?;
                    Some(palette)
                }
                (IMAGE_DATA, _) => return Err(Error::new(InvalidData, "data chunk before palette")),
                (IMAGE_END, _) => return Err(Error::new(InvalidData, "end chunk before palette")),
                (_, length) => {
                    self.ignore_chunk(length)?;
                    None
                }
            };
        }
        Ok(())
    }

    fn next_idat(&mut self, consecutive: bool) -> Result<u32> {
        loop {
            match self.prepare_chunk() {
                Ok((IMAGE_DATA, length)) => {
                    log::trace!("located png idat chunk");
                    return Ok(length);
                }
                Ok((IMAGE_END, _)) => {
                    log::trace!("located png end chunk");
                    return Ok(0);
                }
                Ok((_, length)) => {
                    if consecutive {
                        return Err(Error::new(InvalidData, "non-consecutive idat chunk"));
                    } else {
                        self.ignore_chunk(length)?;
                    }
                }
                Err(err) => return Err(err),
            };
        }
    }

    /// Read and ignore the entire chunk + checksum.
    pub(crate) fn ignore_chunk(&mut self, length: u32) -> Result<()> {
        let len = length;
        let mut byte = [0; 1];
        for _ in 0..len {
            self.reader.read_exact(&mut byte)?;
        }
        self.ignore_crc()?;
        Ok(())
    }

    /// Get a u8 out of the reader.
    pub(crate) fn u8(&mut self) -> Result<u8> {
        let mut byte = [0; 1];
        self.reader.read_exact(&mut byte).map_err(Error::from)?;
        Ok(byte[0])
    }

    /// Get a u32 out of the reader
    pub(crate) fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_be_bytes([self.u8()?, self.u8()?, self.u8()?, self.u8()?]))
    }

    /// Ignore the CRC bytes for performance.
    pub(crate) fn ignore_crc(&mut self) -> Result<u32> { self.u32() }

    // Reads u8 bytes from the png idat chunk into the idat_buffer
    // If the idat chunk is exhausted then ignore the checksum and
    // move to the next idat chunk and continue.
    fn fill_idat_buffer(&mut self, length: usize) -> Result<u32> {
        let mut read = 0;
        for _ in 0..length {
            if self.idat_remaining <= 0 {
                log::trace!("seeking another png idat");
                self.idat_remaining = match self.next_idat(true) {
                    Ok(length) => length,
                    Err(err) => {
                        log::warn!("png idat find error: {:?}", err);
                        0
                    }
                };
                if self.idat_remaining <= 0 {
                    break;
                }
            }
            match self.u8() {
                Ok(byte) => {
                    read += 1;
                    self.idat_buffer.push(byte);
                    self.idat_remaining -= 1;
                    if self.idat_remaining <= 0 {
                        self.ignore_crc()?;
                    }
                }
                Err(err) => {
                    log::warn!("idat read error: {:?}", err);
                    break;
                }
            };
        }
        Ok(read)
    }

    // Inflates u8 bytes from the idat_buffer into inflated.
    // Attempts to refill the idat_buffer ready for the next call.
    // Note: this fn() updates buffer, inflated_length & idat_buffer
    fn inflate(&mut self) -> usize {
        while self.inflated_length <= 0 && self.idat_buffer.len() > 0 {
            let miniz = miniz_oxide::inflate::stream::inflate(
                &mut self.inflate_state,
                self.idat_buffer.as_slice(),
                &mut self.inflated,
                MZFlush::None,
            );
            log::trace!("miniz inflate {:?}", miniz);
            self.inflated_length = miniz.bytes_written;
            self.idat_buffer.drain(..miniz.bytes_consumed);
            match miniz.status {
                Ok(MZStatus::Ok) => {
                    match self.fill_idat_buffer(IDAT_BUFFER_LENGTH - self.idat_buffer.len()) {
                        Ok(_) => (),
                        Err(err) => log::warn!("png idat buffer error {:?}", err),
                    }
                }
                Ok(MZStatus::StreamEnd) => log::trace!("miniz inflate found StreamEnd"),
                Ok(MZStatus::NeedDict) => log::trace!("miniz inflate found NeedDict"),
                Err(err) => log::warn!("miniz inflate error: {:?}", err),
            }
            log::trace!("idat buffer length = {}", self.idat_buffer.len());
        }
        self.inflated_length
    }

    // Checks that the buffer has bytes available and attempts to refill if not.
    // Note: this fn() updates inflated_index, buffer_len & buffer
    fn inflated_length(&mut self) -> usize {
        if self.inflated_index >= self.inflated_length {
            self.inflated_index = 0;
            self.inflated_length = 0;
            self.inflate();
        }
        self.inflated_length
    }

    // Takes the next byte in the buffer as the filter type
    // Call this fn() at the beginning of each png scan line
    // Note: this fn() updates inflated_index, byte_index & filter_type
    fn set_filter_type(&mut self) {
        self.filter_type = match self.inflated_length() {
            0 => 0,
            _ => {
                let filter_type = self.inflated[self.inflated_index];
                self.inflated_index += 1;
                if filter_type <= 4 {
                    log::trace!(
                        "png decode line {} filter type = {}",
                        self.byte_index / self.bytes_per_line,
                        filter_type
                    );
                } else {
                    log::warn!("png decode: invalid filter_type={}", filter_type);
                }
                self.byte_index += 1;
                filter_type
            }
        }
    }

    // Get an inflated byte from the png idat
    // Note: this fn() updates inflated_index, byte_index, line_index & filter_type
    // a scanline of rgb begins with a filterbyte FRGBRGBRGBRGB...
    fn get_inflated_byte(&mut self) -> Option<u8> {
        match self.inflated_length() {
            0 => None,
            _ => {
                let byte = self.inflated[self.inflated_index];
                self.inflated_index += 1;
                self.byte_index += 1;
                self.line_index = match self.byte_index % self.bytes_per_line {
                    // capture the filter byte at the beginning of each png scan line
                    0 => {
                        self.set_filter_type();
                        self.prior_px = vec![0u8; self.bytes_per_pixel];
                        0
                    }
                    i => i - 1,
                };
                Some(byte)
            }
        }
    }

    // in the notation from https://www.w3.org/TR/PNG/#9Filters
    // note that a & c relate to the equivalent byte in the prior pixel
    // prior_line   R G B R G B c G B b G B R G B
    // current_line R G B R G B a G B x
    fn unfilter(&mut self) -> Option<u8> {
        let filter = self.filter_type;
        let index = self.line_index;
        let bpp = self.bytes_per_pixel;
        let fx = match self.get_inflated_byte() {
            Some(byte) => byte,
            None => return None,
        };
        let (a, b, c) = if index < bpp {
            (0, self.prior_line[index], 0)
        } else {
            (self.prior_px[index % bpp], self.prior_line[index], self.prior_line[index - bpp])
        };
        let x = match filter {
            0 => fx,
            1 => fx.wrapping_add(a),
            2 => fx.wrapping_add(b),
            3 => fx.wrapping_add(average(a, b)),
            4 => fx.wrapping_add(paeth(a, b, c)),
            _ => fx,
        };
        if index > bpp {
            self.prior_line[index - bpp] = a;
        }
        self.prior_px[index % bpp] = x;
        Some(x)
    }
}

// in the notation from https://www.w3.org/TR/PNG/#9Filters
fn average(a: u8, b: u8) -> u8 { ((a as u16 + b as u16) >> 1) as u8 }

// in the notation from https://www.w3.org/TR/PNG/#9Filters
fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let (a, b, c): (i16, i16, i16) = (a as i16, b as i16, c as i16);
    let p = a + b - c;
    let pa = (p - a).abs();
    let pb = (p - b).abs();
    let pc = (p - c).abs();
    if pa <= pb && pa <= pc {
        a as u8
    } else if pb <= pc {
        b as u8
    } else {
        c as u8
    }
}
