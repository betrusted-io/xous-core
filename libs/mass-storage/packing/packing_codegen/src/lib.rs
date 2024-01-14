// Vendored from https://github.com/stm32-rs/stm32-usbd tag v0.6.0
// Original copyright (c) 2021 Matti Virkkunen <mvirkkunen@gmail.com>, Vadim Kaushan <admin@disasm.info>,
// Nicolas Stalder <n@stalder.io>", Jonas Martin <lichtfeind@gmail.com>
// SPDX-License-Identifier: MIT
// SPDX-LIcense-Identifier: Apache 2.0


#![cfg_attr(feature = "diagnostic-notes", feature(proc_macro_diagnostic))]

extern crate proc_macro;
use self::proc_macro::TokenStream;

use quote::{ quote, format_ident };
use syn::{
    spanned::Spanned,
    Ident,
    Error,
    parse_macro_input,
    Data,
    DeriveInput,
    Fields,
    Attribute,
    Path,
    Meta,
    Lit,
    NestedMeta,
    MetaNameValue,
    Type,
    Variant,
    Expr,
    ExprLit,
    LitInt,
};
use std::fmt::{
    Debug,
    Formatter,
    Result as FmtResult,
};

use proc_macro2::Span;

#[cfg(feature = "diagnostic-notes")]
use proc_macro::{ Level, Diagnostic };

const ATTR_LITTLE_ENDIAN: &str = "little_endian";
const ATTR_BIG_ENDIAN: &str = "big_endian";
const ATTR_MSB0: &str = "msb0";
const ATTR_LSB0: &str = "lsb0";
const ATTR_BYTES: &str = "bytes";
const ATTR_WIDTH: &str = "width";
const ATTR_SPACE: &str = "space";
const ATTR_START_BYTE: &str = "start_byte";
const ATTR_END_BYTE: &str = "end_byte";
const ATTR_START_BIT: &str = "start_bit";
const ATTR_END_BIT: &str = "end_bit";
const PACKED_ATTR: &str = "packed";
const PKD_ATTR: &str = "pkd";


/// Derive for [Packed](../packing/trait.Packed.html)
///
/// # Attributes
///
/// ## Struct level
///
/// Optional flags for `packed` attribute when used at the struct level:
///
/// | Name          | Description |
/// |---------------|-------------|
/// | little_endian | (default) sets the struct default endianness to little endian |
/// | big_endian    | sets the struct default endianness to big endian |
/// | lsb0          | (default) sets the struct bit ordering such that the least significant bit is bit 0 and the most significant bit is bit 7 |
/// | msb0          | sets the struct bit ordering such that the most significant bit is bit 0 and the least significant bit is 7 |
///
/// ## Field level
///
/// Optional parameters for `packed` attribute when used at the field level:
///
/// | Name          | Description | Default |
/// |---------------|-------------|---------|
/// | start_byte    | Zero based offset from the start of the struct where this field starts | Inferred from the end of the previous field |
/// | end_byte      | Zero based offset from the start of the struct where this field ends (inclusive) | Inferred from start_byte + field width |
/// | start_bit     | The bit where this field starts within the start byte. lsb0/msb0 flips the range of this field (7 to 0 vs 0 to 7) | 7 for lsb0, 0 for msb0 |
/// | end_bit       | The bit where this field ends within the end byte (inclusive). lsb0/msb0 flips the range of this field (7 to 0 vs 0 to 7) | 0 for lsb0, 7 for msb0 |
/// | width         | (partially tested) The width of the field in bits. This is checked against start/end byte/bit if they are specified | Inferred from start/end byte/bit and/or the native width of the field type |
/// | space         | (partially tested) The space before the field in bits. Allows shifting a field along by a number of bits | 0 |
///
/// Mandatory values for `pkd` attribute:
///
/// | Index | Description |
/// |-------|-------------|
/// | 0     | start_byte  |
/// | 1     | end_byte    |
/// | 2     | start_bit   |
/// | 3     | end_bit     |
///
/// ## Note about width/space
///
/// The main use case of this macro is explicitly specifying `start_byte`, `end_byte`, `start_bit` and `end_bit` on
/// every field. For SCSI and USB specifications, these values can be read off the tables provided more easily
/// than trying to work out how many bits the field is total, adding offsets, etc, etc. I found transcribing
/// these values directly from the spec much less error prone than attempting to do maths around offsets and
/// trying to remember the exact algorithm the packing macro(s - I tried several) used.
///
/// However, this derive also supports inferring field alignments from the width of the field type, where the
/// last field ended, etc. This is provided so projects can use it for the complex explicit case mentioned above
/// but also use it for the more trivial alignments you'd expect from repr(C) or repr(Packed). `width` and
/// `space` were added to allow the case where most of the struct is as you'd expect but a handful of fields
/// are slightly different. This was working at the time of implementation but has no tests around it currently
/// so may get broken. //TODO: add tests for all supported cases.

#[proc_macro_derive(Packed, attributes(packed, pkd))]
pub fn packed_derive(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    inner(input)
        .unwrap_or_else(|e| e.to_compile_error().into())
}

#[derive(Debug)]
enum Attr {
    Flag { name: Ident, span: Span },
    Value { name: Ident, value: Lit, span: Span },
    Lit { _value: Lit, span: Span },
}

impl Attr {
    fn span(&self) -> Span {
        match self {
            Attr::Flag { span, .. } => span.clone(),
            Attr::Value { span, .. } => span.clone(),
            Attr::Lit { span, .. } => span.clone(),
        }
    }
}

fn get_single_segment(path: &Path) -> Result<Ident, Error> {
    if path.segments.len() != 1 {
        Err(Error::new(path.span(), "Expected 1 segments"))?
    }
    Ok(path.segments[0].ident.clone())
}

fn flatten_attrs(attrs: &Vec<Attribute>) -> Result<Vec<Attr>, Error> {
    let mut ret = Vec::new();

    for a in attrs.iter() {
        match a.parse_meta() {
            Ok(Meta::List(l)) => {
                if l.path.is_ident(PACKED_ATTR) {
                    for n in l.nested.iter() {
                        ret.push(match n {
                            NestedMeta::Meta(Meta::Path(p)) =>
                                Attr::Flag { name: get_single_segment(p)?, span: p.span() },
                            NestedMeta::Meta(Meta::NameValue(MetaNameValue { path, lit, .. })) =>
                                Attr::Value { name: get_single_segment(path)?, value: lit.clone(), span: path.span() },
                            NestedMeta::Lit(l) =>
                                Attr::Lit { _value: l.clone(), span: a.span() },
                            y => panic!("y: {:?}", y),
                        });
                    }
                } else if l.path.is_ident(PKD_ATTR) {
                    if l.nested.len() != 4 {
                        Err(Error::new(l.path.span(), "pkd abbreviated attribute expects exactly 4 values: #[pkd(<start_bit>, <end_bit>, <start_byte>, <end_byte>)]"))?;
                    }

                    let names = vec!["start_bit", "end_bit", "start_byte", "end_byte"];
                    let values: Vec<&NestedMeta> = l.nested.iter().collect();
                    for i in 0..4 {
                        let v = values[i];
                        if let NestedMeta::Lit(lit) = v {
                            ret.push(Attr::Value {
                                name: Ident::new(names[i], lit.span()),
                                value: lit.clone(),
                                span: lit.span(),
                            });
                        } else {
                            Err(Error::new(l.path.span(), "pkd abbreviated attribute expects exactly 4 values: #[pkd(<start_bit>, <end_bit>, <start_byte>, <end_byte>)]"))?;
                        }
                    }
                }
            },
            // #[packed] with no extra attributes
            Ok(Meta::Path(_)) => {},
            // #[doc] or similar
            Ok(Meta::NameValue(_m)) => {},
            x => panic!("x: {:?}", x),
        }
    }

    Ok(ret)
}

trait Name {
    fn name() -> &'static str;
    fn instance_name(&self) -> &'static str;
}

trait TryFrom<T> {
    fn try_from(v: &T) -> Result<Self, Error> where Self: Sized;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Endian {
    Big,
    Little,
}
impl Endian {
    fn to_packing_endian_tokenstream(&self) -> proc_macro2::TokenStream {
        match self {
            Endian::Little => quote! { packing::LittleEndian },
            Endian::Big => quote! { packing::BigEndian },
        }
    }
}
impl Default for Endian {
    fn default() -> Endian {
        Endian::Little
    }
}
impl Name for Endian {
    fn name() -> &'static str {
        "Endian"
    }
    fn instance_name(&self) -> &'static str {
        match self {
            Endian::Big => "big",
            Endian::Little => "little",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BitOrder {
    Msb0,
    Lsb0,
}
impl Default for BitOrder {
    fn default() -> BitOrder {
        BitOrder::Lsb0
    }
}
impl Name for BitOrder {
    fn name() -> &'static str {
        "BitOrder"
    }
    fn instance_name(&self) -> &'static str {
        match self {
            BitOrder::Msb0 => "Msb0",
            BitOrder::Lsb0 => "Lsb0",
        }
    }
}
impl BitOrder {
    fn map_bits(&self, (b, span): (usize, Span)) -> (usize, Span) {
        let b = match self {
            BitOrder::Lsb0 => 7-b,
            BitOrder::Msb0 => b,
        };
        (b, span)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Scope {
    Struct,
    Field,
}
impl Name for Scope {
    fn name() -> &'static str {
        "Scope"
    }
    fn instance_name(&self) -> &'static str {
        match self {
            Scope::Struct => "Struct",
            Scope::Field => "Field",
        }
    }
}

fn lit_to_usize(lit: &Lit) -> Result<usize, Error> {
    match lit {
        Lit::Int(i) => Ok(i.base10_parse()?),
        _ => Err(Error::new(lit.span(), format!("Expected usize literal but got: {:?}", lit))),
    }
}

impl TryFrom<Attr> for Option<(usize, Span)> {
    fn try_from(v: &Attr) -> Result<Option<(usize, Span)>, Error> {
        match v {
            Attr::Value { value, .. } => Ok(Some((lit_to_usize(value)?, value.span()))),
            _ => Err(Error::new(v.span(), format!("Expected Attr::Value but got: {:?}", v))),
        }
    }
}


macro_rules! usize_field {
    ($type: ident, $name: expr, $instance_name: expr) => {
        #[derive(Clone, Copy, Default)]
        struct $type (Option<(usize, Span)>);
        impl Name for $type {
            fn name() -> &'static str {
                $name
            }
            fn instance_name(&self) -> &'static str {
                $instance_name
            }
        }
        impl TryFrom<Attr> for $type {
            fn try_from(v: &Attr) -> Result<$type, Error> {
                Option::<(usize, Span)>
                    ::try_from(v)
                    .map(|u| Self(u))
            }
        }
        impl Debug for $type {
            fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
                match self.0 {
                    None => write!(f, "Unspecified"),
                    Some((b, ..)) => write!(f, "{} {}", b, self.instance_name()),
                }
            }
        }
        impl $type {
            #[allow(dead_code)]
            fn value(&self) -> Option<usize> {
                self.0.map(|x| x.0)
            }
        }
    }
}

usize_field!(Bytes, "Bytes", "Bytes");
usize_field!(Width, "Width", "Width");
usize_field!(Space, "Space", "Space");
usize_field!(StartByte, "StartByte", "StartByte");
usize_field!(EndByte, "EndByte", "EndByte");
usize_field!(StartBit, "StartBit", "StartBit");
usize_field!(EndBit, "EndBit", "EndBit");


fn get_attr<'a, I, Ta: 'a, Tb, F>(iter: I, _span: Span, scope: Scope, default: Tb, filter_map: F) -> Result<Tb, Error>
where
    I: Iterator<Item = &'a Ta>,
    Tb: Clone + Name + Debug,
    F: FnMut(&Ta) -> Option<(Result<Tb, Error>, Span)>,
{
    let results: Vec<(Tb, Span)> = iter.filter_map(filter_map).map(|r| match r {
        (Ok(r), span) => Ok((r, span)),
        (Err(e), span) => Err(Error::new(span, e)),
    }).collect::<Result<_, _>>()?;

    let scope = scope.instance_name();
    let name = Tb::name();

    //let multi_span: Vec<proc_macro::Span> = vec![span.unwrap()];
    match results.len() {
        0 => {
            let r = default;
            //Diagnostic::spanned(span.unwrap(), Level::Note, format!("{}.{} not specified, defaulting to {:?}", scope, name, r)).emit();
            Ok(r)
        },
        1 => {
            let (r, _span) = results[0].clone();
            //Diagnostic::spanned(span.unwrap(), Level::Note, format!("{}.{} specified as {:?}", scope, name, r)).emit();
            Ok(r)
        },
        _ => {
            #[cfg(feature = "diagnostic-notes")]
            {
                Diagnostic::spanned(results.iter().map(|x| x.1.unwrap()).collect::<Vec<proc_macro::Span>>(),
                    Level::Error, format!("{}.{} specified multiple times", scope, name)).emit();
            }

            Err(Error::new(results[results.len()-1].1, format!("Multiple {}.{} is invalid", scope, name)))
        },
    }
}

fn get_value<'a, A, B>(attrs: A, span: Span, scope: Scope, name_: &str) -> Result<B, Error>
where
    A: Iterator<Item = &'a Attr>,
    B: TryFrom<Attr> + Debug + Clone + Default + Name
{
    get_attr(attrs, span, scope, Default::default(), |a| match a {
        Attr::Value { name, value, .. } if name == name_ => {
            Some((B::try_from(a), value.span()))
        },
        _ => None,
    })
}

fn get_endianness<'a, A>(attrs: A, span: Span, scope: Scope, default: Endian) -> Result<Endian, Error>
where
    A: Iterator<Item = &'a Attr>
{
    get_attr(attrs, span, scope, default, |a| match a {
        Attr::Flag { name, span } if name == ATTR_LITTLE_ENDIAN  => Some((Ok(Endian::Little), span.clone())),
        Attr::Flag { name, span } if name == ATTR_BIG_ENDIAN => Some((Ok(Endian::Big), span.clone())),
        _ => None,
    })
}

fn get_bit_order<'a, A>(attrs: A, span: Span, scope: Scope) -> Result<BitOrder, Error>
where
    A: Iterator<Item = &'a Attr>
{
    get_attr(attrs, span, scope, Default::default(), |a| match a {
        Attr::Flag { name, span } if name == ATTR_MSB0  => Some((Ok(BitOrder::Msb0), span.clone())),
        Attr::Flag { name, span } if name == ATTR_LSB0 => Some((Ok(BitOrder::Lsb0), span.clone())),
        _ => None,
    })
}

const SUPPORTED_FIELD_TYPES: [(&str, usize); 6] = [
    ("bool", 1),
    ("u8", 8),
    ("u16", 16),
    ("u32", 32),
    ("u64", 64),
    ("u128", 128),
];

fn get_next_bigger_type(bits: usize) -> Option<&'static str> {
    SUPPORTED_FIELD_TYPES
        .iter()
        .find(|x| x.1 >= bits)
        .map(|x| x.0)
}

fn get_bit_width(ident: &Ident) -> Option<usize> {
    for (i, size) in SUPPORTED_FIELD_TYPES.iter() {
        if ident.eq(i) {
            return Some(*size);
        }
    }
    None
}

struct Field {
    name: Ident,
    out_bits: Option<usize>,
    out_type: Type,
    width: Width,
    space: Space,
    start_byte: StartByte,
    end_byte: EndByte,
    start_bit: StartBit,
    end_bit: EndBit,
    endian: Endian,
}

struct ExplicitField {
    name: Ident,
    out_type: Type,
    start_bit: usize,
    end_bit: usize,
    endian: Endian,
    width_bytes: usize,
    start_byte: usize,
    end_byte: usize,
}

fn map_typenum(b: usize) -> proc_macro2::TokenStream {
    let ident = format_ident!("U{}", b);
    quote! { packing::#ident }
}

impl ExplicitField {
    fn get_unpacker(&self) -> proc_macro2::TokenStream {
        let sbyte = self.start_byte;
        let ebyte = self.end_byte;

        let sbit = map_typenum(7-(self.start_bit % 8) );
        let ebit = map_typenum(7-(self.end_bit % 8) );
        let endian = self.endian();
        let ty = &self.out_type;

        (quote! {{
            const W: usize = <#ty as packing::PackedSize>::BYTES;
            let mut field_bytes = [0; W];
            <#endian as packing::Endian>::align_field_bits::<#sbit, #ebit>(&bytes[#sbyte..=#ebyte], &mut field_bytes);
            <#ty as packing::PackedBytes<[u8; W]>>::from_bytes::<#endian>(field_bytes)?
        }}).into()
    }
    fn get_packer(&self) -> proc_macro2::TokenStream {
        let sbyte = self.start_byte;
        let ebyte = self.end_byte;

        let sbit = map_typenum(7-(self.start_bit % 8) );
        let ebit = map_typenum(7-(self.end_bit % 8) );
        let endian = self.endian();
        let ty = &self.out_type;
        let name = &self.name;

        (quote! {{
            const W: usize = <#ty as packing::PackedSize>::BYTES;
            let field_bytes = <#ty as packing::PackedBytes<[u8; W]>>::to_bytes::<#endian>(&self.#name)?;
            <#endian as packing::Endian>::restore_field_bits::<#sbit, #ebit>(&field_bytes, &mut bytes[#sbyte..=#ebyte]);
        }}).into()
    }

    fn endian(&self) -> proc_macro2::TokenStream {
        self.endian.to_packing_endian_tokenstream()
    }

    fn get_pack_pair(&self) -> (proc_macro2::TokenStream, proc_macro2::TokenStream) {
        let unpacker = self.get_unpacker();
        let packer = self.get_packer();
        let name = &self.name;
        let width_bytes = self.width_bytes;
        let sbyte = self.start_byte;
        let ebyte = self.end_byte;

        match &self.out_type {
            Type::Path(_p) => (
                quote! { #packer; },
                quote! { #name: #unpacker, },
            ),
            Type::Array(a) => {
                match &*a.elem {
                    Type::Path(p) => {
                        if !p.path.is_ident("u8") {
                            panic!("Only u8 arrays supported ({:?})", p.path);
                        }

                        (
                            quote! { bytes[#sbyte..=#ebyte].copy_from_slice(&self.#name); },
                            quote! { #name: {
                                let mut t = [0; #width_bytes];
                                t.copy_from_slice(&bytes[#sbyte..=#ebyte]);
                                t
                            }, }
                        )
                    },
                    other => panic!("Unsupported array element type: {:?}", other),
                }
            },
            other => panic!("Unhandled out type {:?}", other),
        }
    }
}

impl Debug for Field {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        write!(f, r"Field {{
    name: {},
    out_bits: {:?},
    width: {:?},
    space: {:?},
    start_byte: {:?},
    end_byte: {:?},
    start_bit: {:?},
    end_bit: {:?},
}}
",
        self.name, self.out_bits, self.width.value(), self.space.value(), self.start_byte.value(),
        self.end_byte.value(), self.start_bit.value(), self.end_bit.value())
    }
}

fn error_or_diagnostic<M: core::fmt::Display>(span: Span, msg: M) -> Result<(), Error> {
    #[cfg(feature = "diagnostic-notes")]
    {
        Diagnostic::spanned(span.unwrap(), Level::Error, msg).emit();
        return Ok(());
    }

    #[cfg(not(feature = "diagnostic-notes"))]
    {
        return Err(Error::new(span, msg));
    }

}

fn derive_struct(
    struct_span: Span,
    struct_ident: Ident,
    bit_order: BitOrder,
    struct_endian: Endian,
    fields: Fields,
) -> Result<TokenStream, Error>
{
    let named_fields = if let Fields::Named(f) = fields {
        f
    } else {
        // TODO: shouldn't be hard to support the other kinds
        Err(Error::new(struct_span, "Only named struct fields are accepted currently"))?
    };

    let mut fields = Vec::new();

    for f in named_fields.named {
        let attrs = flatten_attrs(&f.attrs)?;

        let (ty, width) = match &f.ty {
            Type::Path(tp) => (f.ty.clone(), get_bit_width(tp.path.get_ident().unwrap())),
            Type::Array(_a) => (f.ty.clone(), None),
            other => Err(Error::new(f.ident.span(), format!("Only Type::Path & Type::Array supported ({:?})", other)))?,
        };

        let mut field = Field {
            name: f.ident.clone().unwrap(), // Since we checked it's a named struct above this is ok
            out_bits: width,
            out_type: ty,
            width: get_value(attrs.iter(), f.ident.span(), Scope::Field, ATTR_WIDTH)?,
            space: get_value(attrs.iter(), f.ident.span(), Scope::Field, ATTR_SPACE)?,
            start_byte: get_value(attrs.iter(), f.ident.span(), Scope::Field, ATTR_START_BYTE)?,
            end_byte: get_value(attrs.iter(), f.ident.span(), Scope::Field, ATTR_END_BYTE)?,
            start_bit: get_value(attrs.iter(), f.ident.span(), Scope::Field, ATTR_START_BIT)?,
            end_bit: get_value(attrs.iter(), f.ident.span(), Scope::Field, ATTR_END_BIT)?,
            endian: get_endianness(attrs.iter(), f.ident.span(), Scope::Field, struct_endian)?,
        };


        if let Some(eb) = field.end_bit.value() {
            if eb > 7 {
                Err(Error::new(field.end_bit.0.unwrap().1,
                    "end_bit must be between 0 and 7 (inclusive)"))?;
            } else {
                field.end_bit.0 = field.end_bit.0.map(|b| bit_order.map_bits(b));
            }
        }
        if let Some(sb) = field.start_bit.value() {
            if sb > 7 {
                Err(Error::new(field.start_bit.0.unwrap().1,
                    "start_bit must be between 0 and 7 (inclusive)"))?;
            } else {
                field.start_bit.0 = field.start_bit.0.map(|b| bit_order.map_bits(b));
            }
        }

        if let (Some(sby), Some(eby)) = (field.start_byte.value(), field.end_byte.value()) {
            if sby > eby {
                Err(Error::new(field.start_byte.0.unwrap().1,
                    "start_byte must be <= end_byte"))?
            }
        }

        if let (Some(sb), Some(eb)) = (field.start_bit.value(), field.end_bit.value()) {
            if let (Some(sby), Some(eby)) = (field.start_byte.value(), field.end_byte.value()) {
                if sb > eb && sby == eby {
                    if bit_order == BitOrder::Lsb0 {
                        Err(Error::new(field.start_bit.0.unwrap().1,
                            "start_bit < end_bit && start_byte == end_byte is not valid when using lsb0 bit order"))?;
                    } else {
                        Err(Error::new(field.start_bit.0.unwrap().1,
                            "start_bit > end_bit && start_byte == end_byte is not valid when using msb0 bit order"))?;
                    }
                }
            }
        }

        fields.push(field);
    }

    let mut pack_to_comment = "|byte|".to_string();
    match bit_order {
        BitOrder::Msb0 => for i in 0..=7 {
            pack_to_comment += &format!("{}|", i);
        },
        BitOrder::Lsb0 => for i in (0..=7_usize).rev() {
            pack_to_comment += &format!("{}|", i);
        },
    }
    pack_to_comment += "\n|-|-|-|-|-|-|-|-|-|\n";


    let mut explicit_fields = Vec::new();
    let mut bit = 0;

    let mut max_byte = 0;

    for f in fields {
        if let Some(b) = f.start_bit.value() {
            while bit % 8 != b {
                bit += 1;
            }
        }

        if let Some(b) = f.start_byte.value() {
            if b < bit / 8 {
                error_or_diagnostic(f.name.span(),
                    format!("Field start ({}) specified before current position ({}), are the fields out of order?",
                        b, bit/8))?;
            }
            while b > bit / 8 {
                bit += 8;
            }
        }

        let mut end = bit;
        let mut end_set = false;
        if let Some(b) = f.end_bit.value() {
            while end % 8 != b {
                end += 1;
            }
            end_set = true;
        }

        if let Some(b) = f.end_byte.value() {
            while b > end / 8 {
                end += 8;
            }
            end_set = true;
        }

        if let Some(w) = f.width.value() {
            if end_set {
                if w != end - bit {
                    error_or_diagnostic(f.name.span(),
                        format!("Field specifies width of {} but calculated width is {}. Check width, start/end byte/bit attributes",
                            w, end - bit))?;
                }
            } else {
                end += w;
                end_set = true;
            }
        }

        if let Some(width) = f.out_bits {
            if !end_set {
                //TODO:
                //end += width;

                #[cfg(feature = "diagnostic-notes")]
                Diagnostic::spanned(f.name.span().unwrap(), Level::Note,
                    format!("Field {} inferred length: {}",
                        f.name, width)).emit();

                panic!("!end_set: {:?}", f);
            }

            if end - bit > width {
                error_or_diagnostic(f.name.span(),
                    format!("Field width is {} bits which is more than will fit in {:?} ({} bits)",
                        end - bit, f.out_type, width))?;
            }
        }

        #[cfg(feature = "diagnostic-notes")]
        Diagnostic::spanned(f.name.span().unwrap(), Level::Note,
            format!("{}: {} -> {} ({}.{} .. {}.{})", f.name, bit, end,
                f.start_byte.value().unwrap(),
                f.start_bit.value().unwrap(),
                f.end_byte.value().unwrap(),
                f.end_bit.value().unwrap(),
            )).emit();

        let start_byte = bit / 8;
        let end_byte = end / 8;
        explicit_fields.push(ExplicitField {
            name: f.name,
            out_type: f.out_type,
            start_bit: bit,
            end_bit: end,
            endian: f.endian,
            width_bytes: end_byte - start_byte + 1,
            start_byte,
            end_byte,
        });

        bit = end;
        max_byte = max_byte.max(end / 8);
    }

    let (lsb, msb) = if bit_order == BitOrder::Lsb0 {
        (" LSB", " MSB")
    } else {
        (" MSB", " LSB")
    };

    bit = 0;
    for f in explicit_fields.iter() {
        for i in bit..=f.end_bit {
            pack_to_comment += "|";
            if i % 8 == 0 {
                pack_to_comment += &format!("{}|", i / 8);
            }
            if i == f.start_bit {
                pack_to_comment += &format!("{}", f.name);
                if f.start_bit != f.end_bit {
                    pack_to_comment += msb;
                }
            } else if i == f.end_bit {
                pack_to_comment += &format!("{}", f.name);
                if f.start_bit != f.end_bit {
                    pack_to_comment += lsb;
                }
            } else if i > f.start_bit && i < f.end_bit {
                pack_to_comment += " - ";
            }

            if i % 8 == 7 {
                pack_to_comment += "|\n";
            }
        }
        bit = f.end_bit + 1;
    }

    let min_len = max_byte + 1;

    pack_to_comment.insert_str(0, &format!("Pack into the provided byte slice.\n\n`bytes.len()` must be at least {}\n\n", min_len));

    let mut unpack_comment = format!("Unpack from byte slice into new instance.\n\n`bytes.len()` must be at least {}\n\n", min_len);
    unpack_comment += &format!("See [pack_to](struct.{}.html#method.pack_to) for layout diagram", struct_ident.to_string());

    let mut unpack_to_self = format!("Unpack from byte slice into self.\n\n`bytes.len()` must be at least {}\n\n", min_len);
    unpack_to_self += &format!("See [pack_to](struct.{}.html#method.pack_to) for layout diagram", struct_ident.to_string());

    //let pack_bytes_len_comment = format!("Number of bytes this struct packs to/from ({})", min_len);

    let mut unpackers = Vec::new();
    let mut packers = Vec::new();

    for f in explicit_fields.iter() {
        let (packer, unpacker) = f.get_pack_pair();

        unpackers.push(unpacker);
        packers.push(packer);
    }

    let result = quote!{
        impl packing::Packed for #struct_ident {
            type Error = packing::Error;

            #[doc = #pack_to_comment]
            fn pack(&self, bytes: &mut [u8]) -> Result<(), Self::Error> {
                if bytes.len() < #min_len {
                    return Err(packing::Error::InsufficientBytes);
                }

                // TODO: Remove this once `Endian::restore_field_bits` clears bits inside the fields
                //       currently for non-aligned fields it will be ORing into a dirty buffer unless
                //       consumer clears before calling this.
                for b in bytes[0..#min_len].iter_mut() { *b = 0 }


                #( #packers )*

                Ok(())
            }
            fn unpack(bytes: &[u8]) -> Result<Self, Self::Error> {
                if bytes.len() < #min_len {
                    return Err(packing::Error::InsufficientBytes);
                }

                Ok(Self {
                    #( #unpackers )*
                })
            }
        }

        impl packing::PackedBytes<[u8; #min_len]> for #struct_ident {
            type Error = packing::Error;
            fn to_bytes<En: packing::Endian>(&self) -> Result<[u8; #min_len], Self::Error> {
                let mut res = [0; #min_len];
                packing::Packed::pack(self, &mut res)?;
                Ok(res)
            }
            fn from_bytes<En: packing::Endian>(bytes: [u8; #min_len]) -> Result<Self, Self::Error> {
                Self::unpack(&bytes)
            }
        }

        impl packing::PackedSize for #struct_ident {
            const BYTES: usize = #min_len;
        }
    };

    Ok(result.into())
}

fn derive_enum(
    struct_span: Span,
    struct_ident: Ident,
    _bit_order: BitOrder,
    _struct_endian: Endian,
    variants: Vec<Variant>,
) -> Result<TokenStream, Error>
{
    let mut max_discriminant = 0;

    let mut parsed_variants = Vec::new();

    for v in variants.iter() {
        let ident = v.ident.clone();
        if v.fields != Fields::Unit {
            Err(Error::new(ident.span(), "Only unit variants supported by Packed derive"))?;
        }

        if let Some((_, Expr::Lit(ExprLit { lit, .. }))) = v.discriminant.as_ref() {
            let value = lit_to_usize(lit)?;
            max_discriminant = max_discriminant.max(value);

            parsed_variants.push((ident.clone(), value));
        } else {
            Err(Error::new(ident.span(), "Literal expression enum discriminant required for Packed derive (e.g. Variant = 0x1)"))?;
        }
    }

    let mut min_width = 1;
    while max_discriminant > (2_usize.pow(min_width * 8) - 1) {
        min_width += 1;
    }

    let type_ = get_next_bigger_type(min_width as usize * 8)
        .ok_or(Error::new(struct_span, format!("Failed to find field big enough to fit {} byte enum", min_width)))?;

    let ty = format_ident!("{}", type_);

    let mut match_to = Vec::new();
    let mut match_from = Vec::new();

    for v in parsed_variants.iter() {
        let name = &v.0;
        let num_t = LitInt::new(&format!("{}{}", &v.1, ty), name.span());
        match_to.push(quote!  { #num_t => Ok(#struct_ident::#name), });
        match_from.push(quote!{ #struct_ident::#name => #num_t, });
    }

    let width = min_width as usize;

    let mut results = Vec::new();

    results.push(quote!{
        impl #struct_ident {
            pub fn to_primitive(&self) -> #ty {
                match self {
                    #( #match_from )*
                }
            }
            pub fn from_primitive(num: #ty) -> Result<Self, packing::Error>  {
                match num {
                    #( #match_to)*
                    _ => Err(packing::Error::InvalidEnumDiscriminant),
                }
            }
        }

        impl packing::PackedSize for #struct_ident {
            //TODO: enum size > 1 byte
            const BYTES: usize = #width;
        }
    });

    results.push(quote!{
        impl packing::PackedBytes<[u8; #width]> for #struct_ident {
            type Error = packing::Error;
            fn to_bytes<En: packing::Endian>(&self) -> Result<[u8; #width], Self::Error> {
                let num = match self {
                    #( #match_from )*
                };
                Ok(<#ty as packing::PackedBytes<[u8; #width]>>::to_bytes::<En>(&num)?)
            }
            fn from_bytes<En: packing::Endian>(bytes: [u8; #width]) -> Result<Self, Self::Error> {
                let num = <#ty as packing::PackedBytes<[u8; #width]>>::from_bytes::<En>(bytes)?;
                match num {
                    #( #match_to )*
                    _ => Err(packing::Error::InvalidEnumDiscriminant),
                }
            }
        }

    });

    Ok(quote! {
        #( #results )*
    }.into())
}

fn inner(input: DeriveInput) -> Result<TokenStream, Error> {
    let struct_ident = input.ident.clone();
    let struct_span = input.ident.span();

    let struct_attrs = flatten_attrs(&input.attrs)?;
    let struct_endian = get_endianness(struct_attrs.iter(), struct_span, Scope::Struct, Default::default())?;
    let bit_order = get_bit_order(struct_attrs.iter(), struct_span, Scope::Struct)?;
    //TODO: use this to check calculated length
    let _bytes: Bytes = get_value(struct_attrs.iter(), struct_span, Scope::Struct, ATTR_BYTES)?;

    match input.data {
        Data::Struct(d) => derive_struct(struct_span, struct_ident, bit_order, struct_endian, d.fields),
        Data::Enum(e) => derive_enum(struct_span, struct_ident, bit_order, struct_endian, e.variants.into_iter().collect()),
        other => Err(Error::new(struct_span, format!("Packed derive only supported on structs ({:?})", other)))?,
    }
}