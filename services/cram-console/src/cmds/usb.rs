use std::fmt::Write;

use cram_hal_service::trng;
use usb_device_xous::{UsbDeviceState, UsbDeviceType, UsbHid};

use crate::{CommonEnv, ShellCmdApi};

#[derive(Debug)]
pub struct Usb {
    usb_dev: UsbHid,
}
impl Usb {
    pub fn new() -> Usb { Usb { usb_dev: UsbHid::new() } }
}

impl<'a> ShellCmdApi<'a> for Usb {
    cmd_api!(usb);

    // inserts boilerplate for command API

    fn process(&mut self, args: String, _env: &mut CommonEnv) -> Result<Option<String>, xous::Error> {
        let mut ret = String::new();
        #[cfg(not(feature = "mass-storage"))]
        let helpstring = "usb [hid] [fido] [debug] [send <string>] [status] [leds] [lock] [unlock] [kbdtest]";
        #[cfg(feature = "mass-storage")]
        let helpstring = "usb [hid] [fido] [ms] [debug] [send <string>] [status] [leds] [lock] [unlock] [kbdtest] [console] [noconsole]";

        let mut tokens = &args.split(' ');

        if let Some(sub_cmd) = tokens.next() {
            match sub_cmd {
                "hid" => {
                    self.usb_dev.ensure_core(usb_device_xous::UsbDeviceType::FidoKbd).unwrap();
                    write!(ret, "USB connected to HID (FIDO + keyboard) core").unwrap();
                }
                #[cfg(feature = "mass-storage")]
                "ms" => {
                    self.usb_dev.ensure_core(usb_device_xous::UsbDeviceType::MassStorage).unwrap();
                    write!(ret, "USB connected to mass storage core").unwrap();
                }
                "fido" => {
                    self.usb_dev.ensure_core(usb_device_xous::UsbDeviceType::Fido).unwrap();
                    write!(ret, "USB connected to FIDO-only core").unwrap();
                }
                "serial" => {
                    self.usb_dev.ensure_core(usb_device_xous::UsbDeviceType::Serial).unwrap();
                    write!(ret, "USB connected to serial core").unwrap();
                }
                "console" => {
                    self.usb_dev.ensure_core(usb_device_xous::UsbDeviceType::Serial).unwrap();
                    // this will enable input injection mode
                    self.usb_dev.serial_console_input_injection();
                    write!(ret, "USB console connected.").ok();
                }
                "noconsole" => {
                    // this will disable any hooks (including the console input hook)
                    self.usb_dev.serial_clear_input_hooks();
                    write!(ret, "USB console disconnected.").ok();
                }
                "trng" => {
                    self.usb_dev.ensure_core(usb_device_xous::UsbDeviceType::Serial).unwrap();
                    let mode = if let Some(sub_cmd) = tokens.next() {
                        match sub_cmd {
                            "raw" => trng::api::TrngTestMode::Raw,
                            _ => trng::api::TrngTestMode::None, // this is the "normal" mode of operation
                        }
                    } else {
                        trng::api::TrngTestMode::None
                    };
                    self.usb_dev.serial_set_trng_mode(mode);
                    write!(ret, "USB TRNG serial sending requested").ok();
                }
                "notrng" => {
                    self.usb_dev.serial_clear_input_hooks();
                    write!(ret, "USB TRNG serial sending should be stopped.").ok();
                }
                "log" => {
                    let level = if let Some(level) = tokens.next() {
                        match level {
                            "trace" => usb_device_xous::LogLevel::Trace,
                            "debug" => usb_device_xous::LogLevel::Debug,
                            "info" => usb_device_xous::LogLevel::Info,
                            "warn" => usb_device_xous::LogLevel::Warn,
                            "err" => usb_device_xous::LogLevel::Err,
                            _ => {
                                log::info!("Valid levels are trace, debug, info, warn, err");
                                ret.write_str("Valid levels are trace, debug, info, warn, err").ok();
                                return Ok(Some(ret));
                            }
                        }
                    } else {
                        log::info!("Valid levels are trace, debug, info, warn, err");
                        ret.write_str("Valid levels are trace, debug, info, warn, err").ok();
                        return Ok(Some(ret));
                    };
                    log::info!("Setting USB log level to {:?}", level);
                    self.usb_dev.set_log_level(level);
                }
                "send" => match self.usb_dev.get_current_core() {
                    Ok(UsbDeviceType::FidoKbd) | Ok(UsbDeviceType::Serial) => {
                        let mut val = String::new();
                        join_tokens(&mut val, &mut tokens);
                        match self.usb_dev.send_str(&val) {
                            Ok(n) => write!(ret, "Sent {} chars", n).unwrap(),
                            Err(_e) => write!(ret, "Can't send: are we connected to a host?").unwrap(),
                        }
                    }
                    Ok(UsbDeviceType::Debug) => {
                        write!(ret, "HID core not connected: please issue 'usb hid' first").unwrap();
                    }
                    _ => write!(ret, "Invalid response checking status").unwrap(),
                },
                "kbdtest" => {
                    let mut test_str = String::new();
                    for c in 0x20..0x7F {
                        // includes a space as the first character
                        // safety - the bounds are checked above in the loop to be the printable ASCII
                        // character range.
                        test_str.push(unsafe { char::from_u32_unchecked(c as u32) });
                    }
                    test_str.push('\n');
                    match self.usb_dev.get_current_core() {
                        Ok(UsbDeviceType::FidoKbd) => match self.usb_dev.send_str(&test_str) {
                            Ok(n) => write!(ret, "Sent {} test string", n).unwrap(),
                            Err(_e) => write!(ret, "Can't send: are we connected to a host?").unwrap(),
                        },
                        Ok(UsbDeviceType::Debug) => {
                            write!(ret, "HID core not connected: please issue 'usb hid' first").unwrap();
                        }
                        _ => write!(ret, "Invalid response checking status").unwrap(),
                    }
                }
                "status" => match self.usb_dev.get_current_core() {
                    Ok(UsbDeviceType::Debug) => write!(ret, "Debug core connected").unwrap(),
                    Ok(UsbDeviceType::FidoKbd) => match self.usb_dev.status() {
                        UsbDeviceState::Configured => write!(ret, "HID core connected to host").unwrap(),
                        UsbDeviceState::Suspend => write!(ret, "HID in suspend").unwrap(),
                        _ => write!(ret, "HID not connected to USB host").unwrap(),
                    },
                    #[cfg(feature = "mass-storage")]
                    Ok(UsbDeviceType::MassStorage) => write!(ret, "USB mass storage connected").unwrap(),
                    _ => write!(ret, "Invalid response checking status").unwrap(),
                },
                "leds" => match self.usb_dev.get_current_core() {
                    Ok(UsbDeviceType::FidoKbd) => match self.usb_dev.get_led_state() {
                        Ok(leds) => write!(ret, "LEDs: {:?}", leds).unwrap(),
                        _ => write!(ret, "Not connected to USB host or other error").unwrap(),
                    },
                    Ok(UsbDeviceType::Debug) => {
                        write!(ret, "HID core not connected: please issue 'usb hid' first").unwrap();
                    }
                    _ => write!(ret, "Invalid response checking status").unwrap(),
                },
                _ => {
                    write!(ret, "{}", helpstring).unwrap();
                }
            }
        } else {
            write!(ret, "{}", helpstring).unwrap();
        }
        Ok(Some(ret))
    }
}

fn join_tokens<'a>(buf: &mut String, tokens: impl Iterator<Item = &'a str>) {
    for (i, tok) in tokens.enumerate() {
        if i == 0 {
            write!(buf, "{}", tok).unwrap();
        } else {
            write!(buf, " {}", tok).unwrap();
        }
    }
}
