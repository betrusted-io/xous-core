use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};

use quick_xml::Reader;
use quick_xml::events::Event;
use quick_xml::name::QName;

#[derive(Debug)]
pub enum ParseError {
    UnexpectedTag,
    MissingValue,
    ParseIntError,
    NonUTF8,
    WriteError,
}

#[derive(Default, Debug)]
pub struct Field {
    #[allow(dead_code)]
    name: String,
    #[allow(dead_code)]
    lsb: usize,
    #[allow(dead_code)]
    msb: usize,
}

#[derive(Default, Debug)]
pub struct Register {
    #[allow(dead_code)]
    name: String,
    #[allow(dead_code)]
    offset: usize,
    #[allow(dead_code)]
    description: Option<String>,
    #[allow(dead_code)]
    fields: Vec<Field>,
}

#[derive(Default, Debug)]
pub struct Interrupt {
    #[allow(dead_code)]
    name: String,
    value: usize,
}

#[derive(Default, Debug)]
pub struct Peripheral {
    name: String,
    pub base: usize,
    #[allow(dead_code)]
    size: usize,
    interrupt: Vec<Interrupt>,
    #[allow(dead_code)]
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
            MissingValue => write!(f, "XML tag should have contained a value"),
            ParseIntError => write!(f, "unable to parse number"),
            NonUTF8 => write!(f, "file is not UTF-8"),
            WriteError => write!(f, "unable to write destination file"),
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
    let contents = reader.read_event_into(&mut buf).map_err(|_| ParseError::UnexpectedTag)?;
    match contents {
        Event::Text(t) => t.unescape().map(|s| s.to_string()).map_err(|_| ParseError::NonUTF8),
        _ => Err(ParseError::UnexpectedTag),
    }
}

fn generate_field<T: BufRead>(reader: &mut Reader<T>) -> Result<Field, ParseError> {
    let mut buf = Vec::new();
    let mut name = None;
    let mut lsb = None;
    let mut msb = None;
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let tag_binding = e.name().as_ref().to_vec();
                let tag_name = std::str::from_utf8(&tag_binding).unwrap();
                match tag_name {
                    "name" => name = Some(extract_contents(reader)?),
                    "lsb" => lsb = Some(parse_usize(extract_contents(reader)?.as_bytes())?),
                    "msb" => msb = Some(parse_usize(extract_contents(reader)?.as_bytes())?),
                    _ => (),
                }
            }
            Ok(Event::End(ref e)) => {
                if let b"field" = e.local_name().as_ref() {
                    break;
                }
            }
            Ok(_) => (),
            Err(e) => panic!("error parsing: {:?}", e),
        }
    }

    Ok(Field {
        name: name.ok_or(ParseError::MissingValue)?,
        lsb: lsb.ok_or(ParseError::MissingValue)?,
        msb: msb.ok_or(ParseError::MissingValue)?,
    })
}

fn generate_fields<T: BufRead>(reader: &mut Reader<T>, fields: &mut Vec<Field>) -> Result<(), ParseError> {
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => match e.local_name().as_ref() {
                b"field" => fields.push(generate_field(reader)?),
                _ => panic!("unexpected tag in <field>: {:?}", e),
            },
            Ok(Event::End(ref e)) => match e.local_name().as_ref() {
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
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let tag_binding = e.local_name().as_ref().to_vec();
                let tag_name = std::str::from_utf8(&tag_binding).map_err(|_| ParseError::NonUTF8)?;
                match tag_name {
                    "name" => name = Some(extract_contents(reader)?),
                    "addressOffset" => offset = Some(parse_usize(extract_contents(reader)?.as_bytes())?),
                    "fields" => generate_fields(reader, &mut fields)?,
                    _ => (),
                }
            }
            Ok(Event::End(ref e)) => {
                if let b"register" = e.local_name().as_ref() {
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
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let tag_binding = e.local_name().as_ref().to_vec();
                let tag_name = std::str::from_utf8(&tag_binding).map_err(|_| ParseError::NonUTF8)?;
                match tag_name {
                    "name" => name = Some(extract_contents(reader)?),
                    "value" => value = Some(parse_usize(extract_contents(reader)?.as_bytes())?),
                    _ => (),
                }
            }
            Ok(Event::End(ref e)) => {
                if let b"interrupt" = e.local_name().as_ref() {
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
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => match e.local_name().as_ref() {
                b"register" => registers.push(generate_register(reader)?),
                _ => panic!("unexpected tag in <registers>: {:?}", e),
            },
            Ok(Event::End(ref e)) => match e.local_name().as_ref() {
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

fn generate_peripheral<T: BufRead>(reader: &mut Reader<T>) -> Result<Peripheral, ParseError> {
    let mut buf = Vec::new();
    let mut name = None;
    let mut base = None;
    let mut size = None;
    let mut registers = vec![];
    let mut interrupts = vec![];
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let tag_binding = e.local_name().as_ref().to_vec();
                let tag_name = std::str::from_utf8(&tag_binding).map_err(|_| ParseError::NonUTF8)?;
                match tag_name {
                    "name" => name = Some(extract_contents(reader)?),
                    "baseAddress" => base = Some(parse_usize(extract_contents(reader)?.as_bytes())?),
                    "size" => size = Some(parse_usize(extract_contents(reader)?.as_bytes())?),
                    "registers" => generate_registers(reader, &mut registers)?,
                    "interrupt" => generate_interrupts(reader, &mut interrupts)?,
                    _ => (),
                }
            }
            Ok(Event::End(ref e)) => {
                if let b"peripheral" = e.local_name().as_ref() {
                    break;
                }
            }
            Ok(_) => (),
            Err(e) => panic!("error parsing: {:?}", e),
        }
    }

    Ok(Peripheral {
        name: name.ok_or(ParseError::MissingValue)?,
        base: base.ok_or(ParseError::MissingValue)?,
        size: size.ok_or(ParseError::MissingValue)?,
        interrupt: interrupts,
        registers,
    })
}

fn generate_peripherals<T: BufRead>(reader: &mut Reader<T>) -> Result<Vec<Peripheral>, ParseError> {
    let mut buf = Vec::new();
    let mut peripherals = vec![];
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => match e.local_name().as_ref() {
                b"peripheral" => peripherals.push(generate_peripheral(reader)?),
                _ => panic!("unexpected tag in <peripherals>: {:?}", e),
            },
            Ok(Event::End(ref e)) => match e.local_name().as_ref() {
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
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let tag_binding = e.local_name().as_ref().to_vec();
                let tag_name = std::str::from_utf8(&tag_binding).map_err(|_| ParseError::NonUTF8)?;
                match tag_name {
                    "name" => name = Some(extract_contents(reader)?),
                    "baseAddress" => base = Some(parse_usize(extract_contents(reader)?.as_bytes())?),
                    "size" => size = Some(parse_usize(extract_contents(reader)?.as_bytes())?),
                    _ => (),
                }
            }
            Ok(Event::End(ref e)) => {
                if let b"memoryRegion" = e.local_name().as_ref() {
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
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => match e.name() {
                QName(b"memoryRegion") => description.memory_regions.push(generate_memory_region(reader)?),
                _ => panic!("unexpected tag in <memoryRegions>: {:?}", e),
            },
            Ok(Event::End(ref e)) => match e.name() {
                QName(b"memoryRegions") => {
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
        match reader.read_event_into(&mut buf) {
            Ok(Event::Empty(ref e)) => match e.name() {
                QName(b"constant") => {
                    let mut constant_descriptor = Constant::default();
                    for maybe_att in e.attributes() {
                        match maybe_att {
                            Ok(att) => {
                                let att_name = String::from_utf8(att.key.local_name().as_ref().into())
                                    .expect("constant: error parsing attribute name");
                                let att_value = String::from_utf8(att.value.to_vec())
                                    .expect("constant: error parsing attribute value");
                                match att_name {
                                    _ if att_name == "name" => constant_descriptor.name = att_value,
                                    _ if att_name == "value" => constant_descriptor.value = att_value,
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
                QName(b"constants") => break,
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
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => match e.name() {
                QName(b"memoryRegions") => parse_memory_regions(reader, description)?,
                QName(b"constants") => generate_constants(reader, description)?,
                _ => panic!("unexpected tag in <vendorExtensions>: {:?}", e),
            },
            Ok(Event::End(ref e)) => match e.name() {
                QName(b"vendorExtensions") => {
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
    let s = r####"// Renode Platform file generated by svd2repl
// This file is automatically generated
cpu: CPU.Betrusted.AesVexRiscv @ sysbus
    cpuType: "rv32imac_zicsr_zifencei"
    privilegeArchitecture: PrivilegeArchitecture.Priv1_10
    PerformanceInMips: 120
"####;
    out.write_all(s.as_bytes())
}

fn print_footer<U: Write>(out: &mut U) -> std::io::Result<()> {
    let s = r####"
abracom_rtc: Timers.Betrusted.ABRTCMC @ i2c 0x68

audio_codec: Sensors.Betrusted.TLV320AIC3100 @ i2c 0x18

flash: SPI.Betrusted.MXIC_MX66UM1G45G @ spinor
    underlyingMemory: spiflash

sysbus:
    init:
        ApplySVD @../utralib/renode/renode.svd
"####;
    out.write_all(s.as_bytes())
}

fn print_memory_regions<U: Write>(
    regions: &[MemoryRegion],
    cs_peripherals: &HashMap<&str, &str>,
    out: &mut U,
) -> std::io::Result<()> {
    writeln!(out, "// Physical base addresses of memory regions")?;
    for region in regions {
        let region_name = region.name.to_lowercase();
        let region_size = if region.size < 4096 {
            4096
        } else {
            if region.size & 4095 != 0 { region.size + 4096 - (region.size & 4095) } else { region.size }
        };

        // Ignore the CSR region, since we explicitly define registers there.
        if region_name == "csr" {
            continue;
        }

        // Ignore any memory region with a name that matches a peripheral, since
        // those regions are handled by the peripheral themselves.
        if cs_peripherals.contains_key(region_name.as_str()) {
            continue;
        }

        writeln!(out, "{}: Memory.MappedMemory @ sysbus 0x{:08x}", region_name, region.base)?;
        writeln!(out, "    size: 0x{:08x}", region_size)?;
        writeln!(out)?;
    }
    writeln!(out)?;
    Ok(())
}

fn print_peripherals<U: Write>(
    peripherals: &[Peripheral],
    regions: &[MemoryRegion],
    cs_peripherals: &HashMap<&str, &str>,
    constants: &[Constant],
    out: &mut U,
) -> std::io::Result<()> {
    writeln!(out, "// Platform Peripherals")?;

    for peripheral in peripherals {
        let lc_name = peripheral.name.to_lowercase();
        // let peripheral_size = if peripheral.size < 4096 {
        //     4096
        // } else {
        //     peripheral.size
        // };
        if let Some(renode_device) = cs_peripherals.get(lc_name.as_str()) {
            writeln!(out, "{}: {} @ sysbus 0x{:08x}", lc_name, renode_device, peripheral.base)?;

            // TODO: Get the frequency from the `constants` table under `CONFIG_CLOCK_FREQUENCY`
            if lc_name == "timer0" {
                let mut freq_found = false;
                for constant in constants {
                    if constant.name == "CONFIG_CLOCK_FREQUENCY" {
                        writeln!(out, "    frequency: {}", constant.value)?;
                        freq_found = true;
                    }
                }
                if !freq_found {
                    panic!("Couldn't discover clock frequency when creating timer0 object");
                }
            } else if lc_name == "com" {
                writeln!(out, "    EC_INTERRUPT -> btevents @ 0")?;
            }

            // If there is a corresponding memory region, add it as parameters.
            for region in regions.iter() {
                if region.name.to_lowercase() == lc_name.as_str() {
                    writeln!(out, "    memAddr: 0x{:x}", region.base)?;
                    writeln!(out, "    memSize: 0x{:x}", region.size)?;
                    break;
                }
            }

            // Note: we skip the `size` value here, since peripherals don't have a known size.

            // Add the interrupt, if one exists.
            if let Some(irq) = peripheral.interrupt.get(0) {
                writeln!(out, "    IRQ -> cpu @ {}", 1000 + irq.value)?;
            }
        } else {
            writeln!(out, "// Unrecognized peripheral: {} @ 0x{:08x}", lc_name, peripheral.base)?;
        }
        writeln!(out)?;
    }
    writeln!(out)?;

    Ok(())
}

pub fn parse_svd<T: Read>(src: T) -> Result<Description, ParseError> {
    let mut buf = Vec::new();
    let buf_reader = BufReader::new(src);
    let mut reader = Reader::from_reader(buf_reader);
    let mut description = Description::default();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => match e.name() {
                QName(b"peripherals") => {
                    description.peripherals = generate_peripherals(&mut reader)?;
                }
                QName(b"vendorExtensions") => {
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

    let mut cs_peripherals = HashMap::new();
    cs_peripherals.insert("app_uart", "UART.LiteX_UART");
    cs_peripherals.insert("console", "UART.LiteX_UART");
    cs_peripherals.insert("btevents", "GPIOPort.Betrusted.BtEvents");
    cs_peripherals.insert("engine", "Miscellaneous.Betrusted.Engine");
    cs_peripherals.insert("com", "SPI.Betrusted.BetrustedSocCom");
    cs_peripherals.insert("i2c", "I2C.Betrusted.BetrustedSocI2C");
    cs_peripherals.insert("jtag", "Miscellaneous.Betrusted.BetrustedJtag");
    cs_peripherals.insert("keyboard", "Input.Betrusted.BetrustedKbd");
    cs_peripherals.insert("keyrom", "Miscellaneous.Betrusted.Keyrom");
    cs_peripherals.insert("memlcd", "Video.Betrusted.BetrustedLCD");
    cs_peripherals.insert("sha512", "Miscellaneous.Betrusted.Sha512");
    cs_peripherals.insert("spinor_soft_int", "Miscellaneous.Betrusted.SpinorSoftInt");
    cs_peripherals.insert("spinor", "SPI.Betrusted.BetrustedSpinor");
    cs_peripherals.insert("susres", "Timers.Betrusted.SusRes");
    cs_peripherals.insert("timer0", "Timers.Betrusted.LiteX_Timer_32");
    cs_peripherals.insert("trng_kernel", "Miscellaneous.Betrusted.BetrustedRNGKernel");
    cs_peripherals.insert("trng_server", "Miscellaneous.Betrusted.BetrustedRNGServer");
    cs_peripherals.insert("ticktimer", "Timers.Betrusted.TickTimer");
    cs_peripherals.insert("uart", "UART.LiteX_UART");
    cs_peripherals.insert("wfi", "Miscellaneous.Betrusted.BetrustedWfi");
    cs_peripherals.insert("wdt", "Timers.Betrusted.BetrustedWatchdog");

    print_header(dest).or(Err(ParseError::WriteError))?;
    print_peripherals(
        &description.peripherals,
        &description.memory_regions,
        &cs_peripherals,
        &description.constants,
        dest,
    )
    .or(Err(ParseError::WriteError))?;
    print_memory_regions(&description.memory_regions, &cs_peripherals, dest)
        .or(Err(ParseError::WriteError))?;
    print_footer(dest).or(Err(ParseError::WriteError))?;

    Ok(())
}
