use quick_xml::events::{attributes::Attribute, Event};
use quick_xml::Reader;
use std::io::{BufRead, BufReader, Read, Write};

#[derive(Debug)]
pub enum ParseError {
    UnexpectedTag,
    MissingValue,
    ParseIntError,
    NonUTF8,
    WriteError,
    UnexpectedValue,
    MissingBasePeripheral(String),
}

#[derive(Default, Debug, Clone)]
pub struct Field {
    name: String,
    lsb: usize,
    msb: usize,
}

#[derive(Default, Debug, Clone)]
pub struct Register {
    name: String,
    offset: usize,
    description: Option<String>,
    fields: Vec<Field>,
}

#[derive(Default, Debug, Clone)]
pub struct Interrupt {
    name: String,
    value: usize,
}

#[derive(Default, Debug)]
pub struct Peripheral {
    name: String,
    pub base: usize,
    _size: usize,
    interrupt: Vec<Interrupt>,
    registers: Vec<Register>,
}

#[derive(Default, Debug)]
pub struct MemoryRegion {
    pub name: String,
    pub base: usize,
    pub size: usize,
}

#[derive(Default, Debug)]
pub struct Constant {
    pub name: String,
    pub value: String,
}

#[derive(Default, Debug)]
pub struct Description {
    pub peripherals: Vec<Peripheral>,
    pub memory_regions: Vec<MemoryRegion>,
    pub constants: Vec<Constant>,
}

impl core::fmt::Display for ParseError {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        use ParseError::*;
        match *self {
            UnexpectedTag => write!(f, "unexpected XML tag encountered"),
            UnexpectedValue => write!(f, "unexpected XML tag value encountered"),
            MissingValue => write!(f, "XML tag should have contained a value"),
            ParseIntError => write!(f, "unable to parse number"),
            NonUTF8 => write!(f, "file is not UTF-8"),
            WriteError => write!(f, "unable to write destination file"),
            MissingBasePeripheral(ref name) => write!(f, "undeclared base peripheral: {}", name),
        }
    }
}

impl std::error::Error for ParseError {}

pub fn get_base(value: &str) -> (&str, u32) {
    if value.starts_with("0x") {
        (value.trim_start_matches("0x"), 16)
    } else if value.starts_with("0X") {
        (value.trim_start_matches("0X"), 16)
    } else if value.starts_with("0b") {
        (value.trim_start_matches("0b"), 2)
    } else if value.starts_with("0B") {
        (value.trim_start_matches("0B"), 2)
    } else if value.starts_with('0') && value != "0" {
        (value.trim_start_matches('0'), 8)
    } else {
        (value, 10)
    }
}

fn parse_usize(value: &[u8]) -> Result<usize, ParseError> {
    let value_as_str = String::from_utf8(value.to_vec()).or(Err(ParseError::NonUTF8))?;
    let (value, base) = get_base(&value_as_str);
    usize::from_str_radix(value, base).or(Err(ParseError::ParseIntError))
}

fn extract_contents<T: BufRead>(reader: &mut Reader<T>) -> Result<String, ParseError> {
    let mut buf = Vec::new();
    let contents = reader
        .read_event(&mut buf)
        .map_err(|_| ParseError::UnexpectedTag)?;
    match contents {
        Event::Text(t) => t
            .unescape_and_decode(reader)
            .map_err(|_| ParseError::NonUTF8),
        _ => Err(ParseError::UnexpectedTag),
    }
}

fn generate_field<T: BufRead>(reader: &mut Reader<T>) -> Result<Field, ParseError> {
    let mut buf = Vec::new();
    let mut name = None;
    let mut lsb = None;
    let mut msb = None;
    let mut bit_offset = None;
    let mut bit_width = None;

    loop {
        match reader.read_event(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let tag_name = e
                    .unescape_and_decode(reader)
                    .map_err(|_| ParseError::NonUTF8)?;
                match tag_name.as_str() {
                    "name" if name.is_none() => name = Some(extract_contents(reader)?),
                    "lsb" => lsb = Some(parse_usize(extract_contents(reader)?.as_bytes())?),
                    "msb" => msb = Some(parse_usize(extract_contents(reader)?.as_bytes())?),
                    "bitRange" => {
                        let range = extract_contents(reader)?;
                        if !range.starts_with('[') || !range.ends_with(']') {
                            return Err(ParseError::UnexpectedValue);
                        }

                        let mut parts = range[1..range.len() - 1].split(':');
                        msb = Some(
                            parts
                                .next()
                                .ok_or(ParseError::UnexpectedValue)?
                                .parse::<usize>()
                                .map_err(|_| ParseError::ParseIntError)?,
                        );
                        lsb = Some(
                            parts
                                .next()
                                .ok_or(ParseError::UnexpectedValue)?
                                .parse::<usize>()
                                .map_err(|_| ParseError::ParseIntError)?,
                        );
                    }
                    "bitWidth" => {
                        bit_width = Some(parse_usize(extract_contents(reader)?.as_bytes())?)
                    }
                    "bitOffset" => {
                        bit_offset = Some(parse_usize(extract_contents(reader)?.as_bytes())?)
                    }
                    _ => (),
                }
            }
            Ok(Event::End(ref e)) => {
                if let b"field" = e.name() {
                    break;
                }
            }
            Ok(_) => (),
            Err(e) => panic!("error parsing: {:?}", e),
        }
    }

    // If no msb/lsb and bitRange tags were encountered then
    // it's possible that the field is defined via
    // `bitWidth` and `bitOffset` tags instead. Let's handle this.
    if lsb.is_none() && msb.is_none() {
        if let (Some(bit_width), Some(bit_offset)) = (bit_width, bit_offset) {
            lsb = Some(bit_offset);
            msb = Some(bit_offset + bit_width - 1);
        }
    }

    Ok(Field {
        name: name.ok_or(ParseError::MissingValue)?,
        lsb: lsb.ok_or(ParseError::MissingValue)?,
        msb: msb.ok_or(ParseError::MissingValue)?,
    })
}

fn generate_fields<T: BufRead>(
    reader: &mut Reader<T>,
    fields: &mut Vec<Field>,
) -> Result<(), ParseError> {
    let mut buf = Vec::new();
    loop {
        match reader.read_event(&mut buf) {
            Ok(Event::Start(ref e)) => match e.name() {
                b"field" => fields.push(generate_field(reader)?),
                _ => panic!("unexpected tag in <field>: {:?}", e),
            },
            Ok(Event::End(ref e)) => match e.name() {
                b"fields" => {
                    // println!("End fields");
                    break;
                }
                e => panic!("unhandled value: {:?}", e),
            },
            Ok(Event::Text(_)) => (),
            e => panic!("unhandled value: {:?}", e),
        }
    }
    Ok(())
}

fn generate_register<T: BufRead>(reader: &mut Reader<T>) -> Result<Register, ParseError> {
    let mut buf = Vec::new();
    let mut name = None;
    let mut offset = None;
    let description = None;
    let mut fields = vec![];
    loop {
        match reader.read_event(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let tag_name = e
                    .unescape_and_decode(reader)
                    .map_err(|_| ParseError::NonUTF8)?;
                match tag_name.as_str() {
                    "name" => name = Some(extract_contents(reader)?),
                    "addressOffset" => {
                        offset = Some(parse_usize(extract_contents(reader)?.as_bytes())?)
                    }
                    "fields" => generate_fields(reader, &mut fields)?,
                    _ => (),
                }
            }
            Ok(Event::End(ref e)) => {
                if let b"register" = e.name() {
                    break;
                }
            }
            Ok(_) => (),
            Err(e) => panic!("error parsing: {:?}", e),
        }
    }

    Ok(Register {
        name: name.ok_or(ParseError::MissingValue)?,
        offset: offset.ok_or(ParseError::MissingValue)?,
        description,
        fields,
    })
}

fn generate_interrupts<T: BufRead>(
    reader: &mut Reader<T>,
    interrupts: &mut Vec<Interrupt>,
) -> Result<(), ParseError> {
    let mut buf = Vec::new();
    let mut name = None;
    let mut value = None;
    loop {
        match reader.read_event(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let tag_name = e
                    .unescape_and_decode(reader)
                    .map_err(|_| ParseError::NonUTF8)?;
                match tag_name.as_str() {
                    "name" => name = Some(extract_contents(reader)?),
                    "value" => value = Some(parse_usize(extract_contents(reader)?.as_bytes())?),
                    _ => (),
                }
            }
            Ok(Event::End(ref e)) => {
                if let b"interrupt" = e.name() {
                    break;
                }
            }
            Ok(_) => (),
            Err(e) => panic!("error parsing: {:?}", e),
        }
    }

    interrupts.push(Interrupt {
        name: name.ok_or(ParseError::MissingValue)?,
        value: value.ok_or(ParseError::MissingValue)?,
    });

    Ok(())
}

fn generate_registers<T: BufRead>(
    reader: &mut Reader<T>,
    registers: &mut Vec<Register>,
) -> Result<(), ParseError> {
    let mut buf = Vec::new();
    loop {
        match reader.read_event(&mut buf) {
            Ok(Event::Start(ref e)) => match e.name() {
                b"register" => registers.push(generate_register(reader)?),
                _ => panic!("unexpected tag in <registers>: {:?}", e),
            },
            Ok(Event::End(ref e)) => match e.name() {
                b"registers" => {
                    break;
                }
                e => panic!("unhandled value: {:?}", e),
            },
            Ok(Event::Text(_)) => (),
            e => panic!("unhandled value: {:?}", e),
        }
    }
    Ok(())
}

fn derive_peripheral(base: &Peripheral, child_name: &str, child_base: usize) -> Peripheral {
    Peripheral {
        name: child_name.to_owned(),
        base: child_base,
        _size: base._size,
        interrupt: base.interrupt.clone(),
        registers: base.registers.clone(),
    }
}

fn generate_peripheral<T: BufRead>(
    base_peripheral: Option<&Peripheral>,
    reader: &mut Reader<T>,
) -> Result<Peripheral, ParseError> {
    let mut buf = Vec::new();
    let mut name = None;
    let mut base = None;
    let mut size = None;
    let mut registers = vec![];
    let mut interrupts = vec![];

    loop {
        match reader.read_event(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let tag_name = e
                    .unescape_and_decode(reader)
                    .map_err(|_| ParseError::NonUTF8)?;
                match tag_name.as_str() {
                    "name" => name = Some(extract_contents(reader)?),
                    "baseAddress" => {
                        base = Some(parse_usize(extract_contents(reader)?.as_bytes())?)
                    }
                    "size" => size = Some(parse_usize(extract_contents(reader)?.as_bytes())?),
                    "registers" => generate_registers(reader, &mut registers)?,
                    "interrupt" => generate_interrupts(reader, &mut interrupts)?,
                    _ => (),
                }
            }
            Ok(Event::End(ref e)) => {
                if let b"peripheral" = e.name() {
                    break;
                }
            }
            Ok(_) => (),
            Err(e) => panic!("error parsing: {:?}", e),
        }
    }

    let name = name.ok_or(ParseError::MissingValue)?;
    let base = base.ok_or(ParseError::MissingValue)?;

    // Derive from the base peripheral if specified
    if let Some(base_peripheral) = base_peripheral {
        Ok(derive_peripheral(base_peripheral, &name, base))
    } else {
        Ok(Peripheral {
            name,
            base,
            _size: size.ok_or(ParseError::MissingValue)?,
            interrupt: interrupts,
            registers,
        })
    }
}

fn generate_peripherals<T: BufRead>(reader: &mut Reader<T>) -> Result<Vec<Peripheral>, ParseError> {
    let mut buf = Vec::new();
    let mut peripherals: Vec<Peripheral> = vec![];

    loop {
        match reader.read_event(&mut buf) {
            Ok(Event::Start(ref e)) => match e.name() {
                b"peripheral" => {
                    let base_peripheral = match e.attributes().next() {
                        Some(Ok(Attribute { key, value })) if key == b"derivedFrom" => {
                            let base_peripheral_name = String::from_utf8(value.to_vec())
                                .map_err(|_| ParseError::NonUTF8)?;

                            let base = peripherals
                                .iter()
                                .find(|p| p.name == base_peripheral_name)
                                .ok_or(ParseError::MissingBasePeripheral(base_peripheral_name))?;

                            Some(base)
                        }
                        _ => None,
                    };

                    peripherals.push(generate_peripheral(base_peripheral, reader)?);
                }
                _ => panic!("unexpected tag in <peripherals>: {:?}", e),
            },
            Ok(Event::End(ref e)) => match e.name() {
                b"peripherals" => {
                    break;
                }
                e => panic!("unhandled value: {:?}", e),
            },
            Ok(Event::Text(_)) => (),
            e => panic!("unhandled value: {:?}", e),
        }
    }
    Ok(peripherals)
}

fn generate_memory_region<T: BufRead>(reader: &mut Reader<T>) -> Result<MemoryRegion, ParseError> {
    let mut buf = Vec::new();
    let mut name = None;
    let mut base = None;
    let mut size = None;

    loop {
        match reader.read_event(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let tag_name = e
                    .unescape_and_decode(reader)
                    .map_err(|_| ParseError::NonUTF8)?;
                match tag_name.as_str() {
                    "name" => name = Some(extract_contents(reader)?),
                    "baseAddress" => {
                        base = Some(parse_usize(extract_contents(reader)?.as_bytes())?)
                    }
                    "size" => size = Some(parse_usize(extract_contents(reader)?.as_bytes())?),
                    _ => (),
                }
            }
            Ok(Event::End(ref e)) => {
                if let b"memoryRegion" = e.name() {
                    break;
                }
            }
            Ok(_) => (),
            Err(e) => panic!("error parsing: {:?}", e),
        }
    }

    Ok(MemoryRegion {
        name: name.ok_or(ParseError::MissingValue)?,
        base: base.ok_or(ParseError::MissingValue)?,
        size: size.ok_or(ParseError::MissingValue)?,
    })
}

fn parse_memory_regions<T: BufRead>(
    reader: &mut Reader<T>,
    description: &mut Description,
) -> Result<(), ParseError> {
    let mut buf = Vec::new();
    loop {
        match reader.read_event(&mut buf) {
            Ok(Event::Start(ref e)) => match e.name() {
                b"memoryRegion" => description
                    .memory_regions
                    .push(generate_memory_region(reader)?),
                _ => panic!("unexpected tag in <memoryRegions>: {:?}", e),
            },
            Ok(Event::End(ref e)) => match e.name() {
                b"memoryRegions" => {
                    break;
                }
                e => panic!("unhandled value: {:?}", e),
            },
            Ok(Event::Text(_)) => (),
            e => panic!("unhandled value: {:?}", e),
        }
    }
    Ok(())
}

fn generate_constants<T: BufRead>(
    reader: &mut Reader<T>,
    description: &mut Description,
) -> Result<(), ParseError> {
    let mut buf = Vec::new();
    loop {
        match reader.read_event(&mut buf) {
            Ok(Event::Empty(ref e)) => match e.name() {
                b"constant" => {
                    let mut constant_descriptor = Constant::default();
                    for maybe_att in e.attributes() {
                        match maybe_att {
                            Ok(att) => {
                                let att_name = String::from_utf8(att.key.to_vec())
                                    .expect("constant: error parsing attribute name");
                                let att_value = String::from_utf8(att.value.to_vec())
                                    .expect("constant: error parsing attribute value");
                                match att_name {
                                    _ if att_name == "name" => constant_descriptor.name = att_value,
                                    _ if att_name == "value" => {
                                        constant_descriptor.value = att_value
                                    }
                                    _ => panic!("unexpected attribute name"),
                                }
                            }
                            _ => panic!("unexpected value in constant: {:?}", maybe_att),
                        }
                    }
                    description.constants.push(constant_descriptor)
                }
                _ => panic!("unexpected tag in <constants>: {:?}", e),
            },
            // note to future self: if Litex goe away from attributes to nested elements, you would want
            // Ok(Event::Start(ref e) => match e.name() ... to descend into the next tag level, and then
            // use the tag_name match and extract_contents methods from other functions to generate
            // the structure.
            // note that the two formats could be mutually exclusively compatible within the same code base:
            // if there are no attributes, the attribute iterator would do nothing; and if there are no
            // child elements, the recursive descent would also do nothing.
            Ok(Event::End(ref e)) => match e.name() {
                b"constants" => break,
                e => panic!("unhandled value: {:?}", e),
            },
            Ok(Event::Text(_)) => (),
            e => panic!("unhandled value: {:?}", e),
        }
    }
    Ok(())
}

fn parse_vendor_extensions<T: BufRead>(
    reader: &mut Reader<T>,
    description: &mut Description,
) -> Result<(), ParseError> {
    let mut buf = Vec::new();
    loop {
        match reader.read_event(&mut buf) {
            Ok(Event::Start(ref e)) => match e.name() {
                b"memoryRegions" => parse_memory_regions(reader, description)?,
                b"constants" => generate_constants(reader, description)?,
                _ => panic!("unexpected tag in <vendorExtensions>: {:?}", e),
            },
            Ok(Event::End(ref e)) => match e.name() {
                b"vendorExtensions" => {
                    break;
                }
                e => panic!("unhandled value: {:?}", e),
            },
            Ok(Event::Text(_)) => (),
            e => panic!("unhandled value: {:?}", e),
        }
    }
    Ok(())
}

fn print_header<U: Write>(out: &mut U) -> std::io::Result<()> {
    let s = r####"
#![allow(dead_code)]
use core::convert::TryInto;
use core::sync::atomic::AtomicPtr;

#[derive(Debug, Copy, Clone)]
pub struct Register {
    /// Offset of this register within this CSR
    offset: usize,
    /// Mask of SVD-specified bits for the register
    mask: usize,
}
impl Register {
    pub const fn new(offset: usize, mask: usize) -> Register {
        Register { offset, mask }
    }
}
#[derive(Debug, Copy, Clone)]
pub struct Field {
    /// A bitmask we use to AND to the value, unshifted.
    /// E.g. for a width of `3` bits, this mask would be 0b111.
    mask: usize,
    /// Offset of the first bit in this field
    offset: usize,
    /// A copy of the register address that this field
    /// is a member of. Ideally this is optimized out by the
    /// compiler.
    register: Register,
}
impl Field {
    /// Define a new CSR field with the given width at a specified
    /// offset from the start of the register.
    pub const fn new(width: usize, offset: usize, register: Register) -> Field {
        // Asserts don't work in const fn yet.
        // assert!(width != 0, "field width cannot be 0");
        // assert!((width + offset) < 32, "field with and offset must fit within a 32-bit value");
        // It would be lovely if we could call `usize::pow()` in a const fn.
        let mask = match width {
            0 => 0,
            1 => 1,
            2 => 3,
            3 => 7,
            4 => 15,
            5 => 31,
            6 => 63,
            7 => 127,
            8 => 255,
            9 => 511,
            10 => 1023,
            11 => 2047,
            12 => 4095,
            13 => 8191,
            14 => 16383,
            15 => 32767,
            16 => 65535,
            17 => 131071,
            18 => 262143,
            19 => 524287,
            20 => 1048575,
            21 => 2097151,
            22 => 4194303,
            23 => 8388607,
            24 => 16777215,
            25 => 33554431,
            26 => 67108863,
            27 => 134217727,
            28 => 268435455,
            29 => 536870911,
            30 => 1073741823,
            31 => 2147483647,
            32 => 4294967295,
            _ => 0,
        };
        Field {
            mask,
            offset,
            register,
        }
    }
}
#[derive(Debug, Copy, Clone)]
pub struct CSR<T> {
    pub base: *mut T,
}
impl<T> CSR<T>
where
    T: core::convert::TryFrom<usize> + core::convert::TryInto<usize> + core::default::Default,
{
    pub fn new(base: *mut T) -> Self {
        CSR { base }
    }
    /// Read the contents of this register
    pub fn r(&self, reg: Register) -> T {
        // prevent re-ordering
        core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);

        let usize_base: *mut usize = unsafe { core::mem::transmute(self.base) };
        unsafe { usize_base.add(reg.offset).read_volatile() }
            .try_into()
            .unwrap_or_default()
    }
    /// Read a field from this CSR
    pub fn rf(&self, field: Field) -> T {
        // prevent re-ordering
        core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);

        let usize_base: *mut usize = unsafe { core::mem::transmute(self.base) };
        ((unsafe { usize_base.add(field.register.offset).read_volatile() } >> field.offset)
            & field.mask)
            .try_into()
            .unwrap_or_default()
    }
    /// Read-modify-write a given field in this CSR
    pub fn rmwf(&mut self, field: Field, value: T) {
        let usize_base: *mut usize = unsafe { core::mem::transmute(self.base) };
        let value_as_usize: usize = value.try_into().unwrap_or_default() << field.offset;
        let previous =
            unsafe { usize_base.add(field.register.offset).read_volatile() } & !(field.mask << field.offset);
        unsafe {
            usize_base
                .add(field.register.offset)
                .write_volatile(previous | value_as_usize)
        };
        // prevent re-ordering
        core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
    }
    /// Write a given field without reading it first
    pub fn wfo(&mut self, field: Field, value: T) {
        let usize_base: *mut usize = unsafe { core::mem::transmute(self.base) };
        let value_as_usize: usize = (value.try_into().unwrap_or_default() & field.mask) << field.offset;
        unsafe {
            usize_base
                .add(field.register.offset)
                .write_volatile(value_as_usize)
        };
        // Ensure the compiler doesn't re-order the write.
        // We use `SeqCst`, because `Acquire` only prevents later accesses from being reordered before
        // *reads*, but this method only *writes* to the locations.
        core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
    }
    /// Write the entire contents of a register without reading it first
    pub fn wo(&mut self, reg: Register, value: T) {
        let usize_base: *mut usize = unsafe { core::mem::transmute(self.base) };
        let value_as_usize: usize = value.try_into().unwrap_or_default();
        unsafe { usize_base.add(reg.offset).write_volatile(value_as_usize) };
        // Ensure the compiler doesn't re-order the write.
        // We use `SeqCst`, because `Acquire` only prevents later accesses from being reordered before
        // *reads*, but this method only *writes* to the locations.
        core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
    }
    /// Zero a field from a provided value
    pub fn zf(&self, field: Field, value: T) -> T {
        let value_as_usize: usize = value.try_into().unwrap_or_default();
        (value_as_usize & !(field.mask << field.offset))
            .try_into()
            .unwrap_or_default()
    }
    /// Shift & mask a value to its final field position
    pub fn ms(&self, field: Field, value: T) -> T {
        let value_as_usize: usize = value.try_into().unwrap_or_default();
        ((value_as_usize & field.mask) << field.offset)
            .try_into()
            .unwrap_or_default()
    }
}

#[derive(Debug)]
pub struct AtomicCsr<T> {
    pub base: AtomicPtr<T>,
}
impl<T> AtomicCsr<T>
where
    T: core::convert::TryFrom<usize> + core::convert::TryInto<usize> + core::default::Default,
{
    pub fn new(base: *mut T) -> Self {
        AtomicCsr {
            base: AtomicPtr::new(base)
        }
    }
    /// In reality, we should wrap this in an `Arc` so we can be truly safe across a multi-core
    /// implementation, but for our single-core system this is fine. The reason we don't do it
    /// immediately is that UTRA also needs to work in a `no_std` environment, where `Arc`
    /// does not exist, and so additional config flags would need to be introduced to not break
    /// that compability issue. If migrating to multicore, this technical debt would have to be
    /// addressed.
    pub fn clone(&self) -> Self {
        AtomicCsr {
            base: AtomicPtr::new(self.base.load(core::sync::atomic::Ordering::SeqCst))
        }
    }
    /// Read the contents of this register
    pub fn r(&self, reg: Register) -> T {
        // prevent re-ordering
        core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);

        let usize_base: *mut usize = unsafe { core::mem::transmute(self.base.load(core::sync::atomic::Ordering::SeqCst)) };
        unsafe { usize_base.add(reg.offset).read_volatile() }
            .try_into()
            .unwrap_or_default()
    }
    /// Read a field from this CSR
    pub fn rf(&self, field: Field) -> T {
        // prevent re-ordering
        core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);

        let usize_base: *mut usize = unsafe { core::mem::transmute(self.base.load(core::sync::atomic::Ordering::SeqCst)) };
        ((unsafe { usize_base.add(field.register.offset).read_volatile() } >> field.offset)
            & field.mask)
            .try_into()
            .unwrap_or_default()
    }
    /// Read-modify-write a given field in this CSR
    pub fn rmwf(&self, field: Field, value: T) {
        let usize_base: *mut usize = unsafe { core::mem::transmute(self.base.load(core::sync::atomic::Ordering::SeqCst)) };
        let value_as_usize: usize = value.try_into().unwrap_or_default() << field.offset;
        let previous =
            unsafe { usize_base.add(field.register.offset).read_volatile() } & !(field.mask << field.offset);
        unsafe {
            usize_base
                .add(field.register.offset)
                .write_volatile(previous | value_as_usize)
        };
        // prevent re-ordering
        core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
    }
    /// Write a given field without reading it first
    pub fn wfo(&self, field: Field, value: T) {
        let usize_base: *mut usize = unsafe { core::mem::transmute(self.base.load(core::sync::atomic::Ordering::SeqCst)) };
        let value_as_usize: usize = (value.try_into().unwrap_or_default() & field.mask) << field.offset;
        unsafe {
            usize_base
                .add(field.register.offset)
                .write_volatile(value_as_usize)
        };
        // Ensure the compiler doesn't re-order the write.
        // We use `SeqCst`, because `Acquire` only prevents later accesses from being reordered before
        // *reads*, but this method only *writes* to the locations.
        core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
    }
    /// Write the entire contents of a register without reading it first
    pub fn wo(&self, reg: Register, value: T) {
        let usize_base: *mut usize = unsafe { core::mem::transmute(self.base.load(core::sync::atomic::Ordering::SeqCst)) };
        let value_as_usize: usize = value.try_into().unwrap_or_default();
        unsafe { usize_base.add(reg.offset).write_volatile(value_as_usize) };
        // Ensure the compiler doesn't re-order the write.
        // We use `SeqCst`, because `Acquire` only prevents later accesses from being reordered before
        // *reads*, but this method only *writes* to the locations.
        core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
    }
    /// Zero a field from a provided value
    pub fn zf(&self, field: Field, value: T) -> T {
        let value_as_usize: usize = value.try_into().unwrap_or_default();
        (value_as_usize & !(field.mask << field.offset))
            .try_into()
            .unwrap_or_default()
    }
    /// Shift & mask a value to its final field position
    pub fn ms(&self, field: Field, value: T) -> T {
        let value_as_usize: usize = value.try_into().unwrap_or_default();
        ((value_as_usize & field.mask) << field.offset)
            .try_into()
            .unwrap_or_default()
    }
}
"####;
    out.write_all(s.as_bytes())
}

fn print_memory_regions<U: Write>(regions: &[MemoryRegion], out: &mut U) -> std::io::Result<()> {
    writeln!(out, "// Physical base addresses of memory regions")?;
    for region in regions {
        writeln!(
            out,
            "pub const HW_{}_MEM:     usize = 0x{:08x};",
            region.name, region.base
        )?;
        writeln!(
            out,
            "pub const HW_{}_MEM_LEN: usize = {};",
            region.name, region.size
        )?;
    }
    writeln!(out)?;
    Ok(())
}

fn print_constants<U: Write>(constants: &[Constant], out: &mut U) -> std::io::Result<()> {
    writeln!(out, "\n// Litex auto-generated constants")?;
    for constant in constants {
        let maybe_intval = constant.value.parse::<u32>();
        match maybe_intval {
            Ok(intval) => {
                writeln!(
                    out,
                    "pub const LITEX_{}: usize = {};",
                    constant.name, intval
                )?;
            }
            Err(_) => {
                writeln!(
                    out,
                    "pub const LITEX_{}: &str = \"{}\";",
                    constant.name, constant.value
                )?;
            }
        }
    }
    writeln!(out)?;
    Ok(())
}

fn print_peripherals<U: Write>(peripherals: &[Peripheral], out: &mut U) -> std::io::Result<()> {
    writeln!(out, "// Physical base addresses of registers")?;
    for peripheral in peripherals {
        writeln!(
            out,
            "pub const HW_{}_BASE :   usize = 0x{:08x};",
            peripheral.name.to_uppercase(),
            peripheral.base
        )?;
    }
    writeln!(out)?;

    let s = r####"
pub mod utra {
"####;
    out.write_all(s.as_bytes())?;

    for peripheral in peripherals {
        writeln!(out)?;
        writeln!(out, "    pub mod {} {{", peripheral.name.to_lowercase())?;
        writeln!(
            out,
            "        pub const {}_NUMREGS: usize = {};",
            peripheral.name.to_uppercase(),
            peripheral.registers.len()
        )?;
        for register in &peripheral.registers {
            writeln!(out)?;
            if let Some(description) = &register.description {
                writeln!(out, "        /// {}", description)?;
            }
            let mut mask: usize = 0;
            for field in &register.fields {
                mask |= ((1 << (field.msb + 1 - field.lsb)) - 1) << field.lsb;
            }
            writeln!(
                out,
                "        pub const {}: crate::Register = crate::Register::new({}, 0x{:x});",
                register.name.to_uppercase(),
                register.offset / 4,
                mask,
            )?;
            for field in &register.fields {
                writeln!(
                    out,
                    "        pub const {}_{}: crate::Field = crate::Field::new({}, {}, {});",
                    register.name,
                    field.name.to_uppercase(),
                    field.msb + 1 - field.lsb,
                    field.lsb,
                    register.name
                )?;
            }
        }
        writeln!(out)?;
        for interrupt in &peripheral.interrupt {
            writeln!(
                out,
                "        pub const {}_IRQ: usize = {};",
                interrupt.name.to_uppercase(),
                interrupt.value
            )?;
        }
        writeln!(
            out,
            "        pub const HW_{}_BASE: usize = 0x{:08x};",
            peripheral.name.to_uppercase(),
            peripheral.base
        )?;
        writeln!(out, "    }}")?;
    }
    writeln!(out, "}}")?;
    Ok(())
}

fn print_tests<U: Write>(peripherals: &[Peripheral], out: &mut U) -> std::io::Result<()> {
    let test_header = r####"
#[cfg(test)]
mod tests {
"####
        .as_bytes();
    out.write_all(test_header)?;

    for peripheral in peripherals {
        let mod_name = peripheral.name.to_lowercase();
        let per_name = peripheral.name.to_lowercase() + "_csr";

        write!(
            out,
            r####"
    #[test]
    #[ignore]
    fn compile_check_{}() {{
        use super::*;
"####,
            per_name
        )?;

        writeln!(
            out,
            "        let mut {} = CSR::new(HW_{}_BASE as *mut u32);",
            per_name,
            peripheral.name.to_uppercase()
        )?;
        for register in &peripheral.registers {
            writeln!(out)?;
            let reg_name = register.name.to_uppercase();
            writeln!(
                out,
                "        let foo = {}.r(utra::{}::{});",
                per_name, mod_name, reg_name
            )?;
            writeln!(
                out,
                "        {}.wo(utra::{}::{}, foo);",
                per_name, mod_name, reg_name
            )?;
            for field in &register.fields {
                let field_name = format!("{}_{}", reg_name, field.name.to_uppercase());
                writeln!(
                    out,
                    "        let bar = {}.rf(utra::{}::{});",
                    per_name, mod_name, field_name
                )?;
                writeln!(
                    out,
                    "        {}.rmwf(utra::{}::{}, bar);",
                    per_name, mod_name, field_name
                )?;
                writeln!(
                    out,
                    "        let mut baz = {}.zf(utra::{}::{}, bar);",
                    per_name, mod_name, field_name
                )?;
                writeln!(
                    out,
                    "        baz |= {}.ms(utra::{}::{}, 1);",
                    per_name, mod_name, field_name
                )?;
                writeln!(
                    out,
                    "        {}.wfo(utra::{}::{}, baz);",
                    per_name, mod_name, field_name
                )?;
            }
        }

        writeln!(out, "  }}")?;
    }
    writeln!(out, "}}")?;

    Ok(())
}

pub fn parse_svd<T: Read>(src: T) -> Result<Description, ParseError> {
    let mut buf = Vec::new();
    let buf_reader = BufReader::new(src);
    let mut reader = Reader::from_reader(buf_reader);
    let mut description = Description::default();
    loop {
        match reader.read_event(&mut buf) {
            Ok(Event::Start(ref e)) => match e.name() {
                b"peripherals" => {
                    description.peripherals = generate_peripherals(&mut reader)?;
                }
                b"vendorExtensions" => {
                    parse_vendor_extensions(&mut reader, &mut description)?;
                }
                _ => (),
            },
            Ok(Event::Eof) => break,
            Err(e) => panic!("Error at position {}: {:?}", reader.buffer_position(), e),
            _ => (),
        }
        buf.clear();
    }
    Ok(description)
}

pub fn generate<T: Read, U: Write>(src: T, dest: &mut U) -> Result<(), ParseError> {
    let description = parse_svd(src)?;

    print_header(dest).or(Err(ParseError::WriteError))?;
    print_memory_regions(&description.memory_regions, dest).or(Err(ParseError::WriteError))?;
    print_peripherals(&description.peripherals, dest).or(Err(ParseError::WriteError))?;
    print_constants(&description.constants, dest).or(Err(ParseError::WriteError))?;
    print_tests(&description.peripherals, dest).or(Err(ParseError::WriteError))?;

    Ok(())
}
