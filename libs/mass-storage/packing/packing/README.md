# packing

[![Crate](https://img.shields.io/crates/v/packing.svg)](https://crates.io/crates/packing)
[![Documentation](https://docs.rs/packing/badge.svg)](https://docs.rs/packing)

This crate provides a `Packed` derive proc macro that generates the code required to pack and unpack a
rust struct to a sequence of bytes.

There are several other crates that provide similar features but the one missing one that this crate
provides is arbitrarily aligned fields - the field needn't start or end at a byte boundary.

Additionally, I wanted to support attributes that closely resemble the way the SCSI and USB specification
documents describe their packets. The field attribute supports a verbose form, where all named attributes
are optional and can appear in any order:

``` #[packed(start_bit=7, end_bit=5)] ```

Or a concise form that requires exactly 4 values (this form matches the afformentioned specs very nicely):

``` #[pkd(7, 5, 0, 0)] ```

The proc macro also generates a nice markdown table showing the field alignments which can be compared to
the original specification easily.

## Example

```
#[derive(Packed)]
pub enum PeripheralQualifier {
    /// A peripheral device having the specified peripheral device type is connected to this logical unit. If the device server is unable to determine whether or not a peripheral device is connected, it also shall use this peripheral qualifier. This peripheral qualifier does not mean that the peripheral device connected to the logical unit is ready for access.
    Connected = 0b000,
    /// A peripheral device having the specified peripheral device type is not connected to this logical unit. However, the device server is capable of supporting the specified peripheral device type on this logical unit.
    NotConnected = 0b001,
    /// The device server is not capable of supporting a peripheral device on this logical unit. For this peripheral qualifier the peripheral device type shall be set to 1Fh. All other peripheral device type values are reserved for this peripheral qualifier.
    Incapable = 0b011,
}
```
```
#[packed(big_endian, lsb0)]
pub struct InquiryResponse {
    #[pkd(7, 5, 0, 0)]
    peripheral_qualifier: PeripheralQualifier,

    #[pkd(4, 0, 0, 0)]
    peripheral_device_type: PeripheralDeviceType,

    ///A removable medium ( RMB ) bit set to zero indicates that the medium is not removable. A RMB bit set to one indicates that the medium is removable.
    #[pkd(7, 7, 1, 1)]
    removable_medium: bool,

    ///The VERSION field indicates the implemented version of this standard and is defined in table 142
    #[pkd(7, 0, 2, 2)]
    version: SpcVersion,

    ///The Normal ACA Supported (NORMACA) bit set to one indicates that the device server supports a NACA bit set to one in the CDB CONTROL byte and supports the ACA task attribute (see SAM-4). A N ORM ACA bit set to zero indicates that the device server does not support a NACA bit set to one and does not support the ACA task attribute.
    #[pkd(5, 5, 3, 3)]
    normal_aca: bool,

    ///The RESPONSE DATA FORMAT field indicates the format of the standard INQUIRY data and shall be set as shown in table 139. A RESPONSE DATA FORMAT field set to 2h indicates that the standard INQUIRY data is in the format defined in this standard. Response data format values less than 2h are obsolete. Response data format values greater than 2h are reserved.
    #[pkd(3, 0, 3, 3)]
    response_data_format: ResponseDataFormat,

    ... additional fields omitted ...
}
```

The rustdoc documentation for the `InquiryResponse` includes the following table:

| byte | 7 | 6 | 5 | 4 | 3 | 2 | 1 | 0 |
|------|---|---|---|---|---|---|---|---|
|0|peripheral_qualifier MSB|-|peripheral_qualifier LSB|peripheral_device_type MSB|-|-|-|peripheral_device_type LSB|
|1|removable_medium|||||||
|2|version MSB|-|-|-|-|-|-|version LSB|
|3|normal_aca|hierarchical_support|response_data_format MSB|-|-|response_data_format LSB|

## License

Free and open source software distributed under the terms of both the [MIT License][lm] and the [Apache License 2.0][la].

[lm]: LICENSE-MIT
[la]: LICENSE-APACHE