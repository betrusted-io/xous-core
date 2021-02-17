#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

/*

This is a server that is used as an "agent" to facilitate EMC compliance testing (e.g. FCC/CE testing).
It's built with the following packages in the cargo xtask hw-image:
    for pkg in &["shell", "graphics-server", "ticktimer-server", "log-server", "com", "fcc-agent"] {

The configuration is very brittle; the system probably won't work with more or less packages. At a
minimum, ticktimer-server, log-server, com, and fcc-agent are mandatory; shell and graphics-server
help give us a notion that the system is running, and also shell is responsible for selecting the
correct UART port for the agent to work.

The overall setup for testing is as follows:

A Raspberry Pi is used to run a Python-based master control script:
https://github.com/betrusted-io/wfx-common-tools/blob/master/test-feature/fcc_test.py

This control scripts issues commands over a UART interface; originally drafted for
a command-ine shell, `fcc-agent` emulates a degenerate shell-like interface, insofar as
parsing and responding to the very limited set of commands allowed by the testing program.

`fcc-agent` requires `betrusted-soc` to have an `app_uart`. This is a third uart (in addition
to the kernel and logging UART). It needs to be muxed to the FPGA UART pins on boot; in this
version of `xous-core`, it is handled by the shell
(see https://github.com/betrusted-io/xous-core/blob/1359db06f4422ea8013d9c2beff4c3017f5346ba/services/shell/src/main.rs#L188).

The basic testing protocol consists of creating custom PDS (platform data set) descriptors
that need to be loaded into the WF200. Normally this is done under Linux by `cat`ing the ASCII
text to the appropriate /sys node. Here we take the PDS ASCII data, and ship it off to the `com`
server, which then packs it into a record that the EC forwards on to the WF200.

Normally, conducted emissions tests are discontinuous, in that they are to be run for
some dozens of seconds, and then turned off, so they can be script-driver. However, for
radiated emissions, the unit must be stand-alone. In this case, the 'repeat' keyword should
be sent, which keeps the transmitter looping indefinitely, and then the device unplugged
from the serial console and put into the chamber for testing.

Therefore, the entire system must be in sync as far as firmware revisions and capabilities.
Here are the git commits of the configuration that was tested and working:

xous-core: 1359db06f4422ea8013d9c2beff4c3017f5346ba
betrusted-soc: 5acef1d9d4d23caf5a0691c8e83e5696afb64879
betrusted-ec: 60f9459876ad42ec251ee5b5f86b85714bcd03bb
com_rs: d61bc3a5e91e2abdeaafc605ebf4f471b025ec7b
betrusted-scripts: 4710f5cbc7fb2c4624b5ebb005d3b47fd7ed5b43
betrusted-scripts/wfx-firmware: 3c6ba6828354a7a158b251f3ebb99a5d7bc59e40 (3.3.2)
wfx-common-tools: cfd4ec82ea53e17c26f8a11c15f6229bc418e8b3

Example run (channel 6, 802.11b, 1Mbps, 40 second burst):

pi@betrusted-dev:~/code/wfx-common-tools/test-feature $ sudo ./fcc_test.py
reset EC
reset SOC
wait for boot...
Serial: Configuring a UART connection using /dev/ttyS0/
user:


Serial connected
wfx_test_agent read_fw_version
got fw_version string: 3.3.2
Serial: fw_version retrieved from HW (3.3.2)
wfx_test_agent --help
Serial: tree filled for FW3.3.2
wfx_test_agent read_agent_version
UART /dev/ttyS0/115200/8/N/1 agent_reply: 1.0.0
UART sending  'wfx_test_agent ec_version'
wfx_test_agent ec_version
UART received <wfx_test_agent ec_version>
UART received <ecrev: 60f94598 clean>
UART sending  'wfx_test_agent soc_version'
wfx_test_agent soc_version
UART received <wfx_test_agent soc_version>
UART received <socrev: 5acef1d9 clean>
UART received <DNA: 005cb5ce5458c854>
Serial   SET|   TEST_IND  1000000
Serial   SET|   FRAME_SIZE_BYTE  4091     IFS_US  0
Serial   SET|   RF_PORTS  TX1_RX1
UART sending  'wfx_test_agent write_test_data  "{j:{a:0}}"'
wfx_test_agent write_test_data  "{j:{a:0}}"
UART received <wfx_test_agent write_test_data  "{j:{a:0}}">
Serial   SET|   TEST_CHANNEL_FREQ  6
Serial   SET|   HT_PARAM  MM     RATE  B_1Mbps
Serial   SET|   TEST_MODE  tx_packet     NB_FRAME  0
UART sending  'wfx_test_agent write_test_data  "{i:{a:6,b:1,f:F4240,c:{a:0,b:0,c:0,d:44},d:{a:FFB,b:0,c:0,d:0,e:0,f:4},e:{}}}"'
wfx_test_agent write_test_data  "{i:{a:6,b:1,f:F4240,c:{a:0,b:0,c:0,d:44},d:{a:FFB,b:0,c:0,d:0,e:0,f:4},e:{}}}"
UART received <wfx_test_agent write_test_data  "{i:{a:6,b:1,f:F4240,c:{a:0,b:0,c:0,d:44},d:{a:FFB,b:0,c:0,d:0,e:0,f:4},e:{}}}">
UART sending  'wfx_test_agent commit_pds'
wfx_test_agent commit_pds
UART received <wfx_test_agent commit_pds>
UART received <{j:{a:0}}>
UART received <{i:{a:6,b:1,f:F4240,c:{a:0,b:0,c:0,d:44},d:{a:FFB,b:0,c:0,d:0,e:0,f:4},e:{}}}>
Serial   SET|   TEST_MODE  tx_packet     NB_FRAME  100
UART sending  'wfx_test_agent write_test_data  "{i:{a:6,b:1,f:F4240,c:{a:0,b:0,c:0,d:44},d:{a:FFB,b:0,c:0,d:0,e:64,f:4},e:{}}}"'
wfx_test_agent write_test_data  "{i:{a:6,b:1,f:F4240,c:{a:0,b:0,c:0,d:44},d:{a:FFB,b:0,c:0,d:0,e:64,f:4},e:{}}}"
UART received <wfx_test_agent write_test_data  "{i:{a:6,b:1,f:F4240,c:{a:0,b:0,c:0,d:44},d:{a:FFB,b:0,c:0,d:0,e:64,f:4},e:{}}}">
UART sending  'wfx_test_agent commit_pds'
wfx_test_agent commit_pds
UART received <wfx_test_agent commit_pds>
UART received <{i:{a:6,b:1,f:F4240,c:{a:0,b:0,c:0,d:44},d:{a:FFB,b:0,c:0,d:0,e:64,f:4},e:{}}}>

*/
use xous::{Message, ScalarMessage};

#[derive(Debug)]
pub enum Opcode<'a> {
    Char(u8),
    RxStats(&'a [u8]),
}
impl<'a> core::convert::TryFrom<&'a Message> for Opcode<'a> {
    type Error = &'static str;
    fn try_from(message: &'a Message) -> Result<Self, Self::Error> {
        match message {
            Message::Scalar(m) => match m.id {
                1 => Ok(Opcode::Char(m.arg1 as u8)),
                _ => Err("unrecognized opcode"),
            },
            Message::Borrow(m) => match m.id {
                2 => {
                    let stats = unsafe {
                        core::slice::from_raw_parts(
                            m.buf.as_ptr(),
                            m.valid.map(|x| x.get()).unwrap_or_else(|| m.buf.len()),
                        )
                    };
                    Ok(Opcode::RxStats(stats))
                }
                _ => {print!("unhandled opcode"); Err("unrecognized opcode")},
            }
            _ => {print!("unhandled message type"); Err("unhandled message type")},
        }
    }
}
impl<'a> Into<Message> for Opcode<'a> {
    fn into(self) -> Message {
        match self {
            Opcode::Char(c) => Message::Scalar(ScalarMessage {
                id: 1, arg1: c as usize, arg2: 0, arg3: 0, arg4: 0}),
            Opcode::RxStats(stats) => {
                let data = xous::carton::Carton::from_bytes(stats);
                Message::Borrow(data.into_message(2))
            }
        }
    }
}

use log::error;
use core::fmt::{Error, Write};

use utralib::generated::*;

use heapless::String;
use heapless::Vec;
use heapless::consts::*;

use core::convert::TryFrom;

use com::*;


pub struct Uart {
    uart_csr: utralib::CSR<u32>,
    rx_conn: xous::CID,
}

// a global static copy of the UART location, must be initialized before use!
pub static mut UART_STRUCT: Uart = Uart {
    uart_csr: utralib::CSR{ base: 0 as *mut u32 },
    rx_conn: 0,
};

#[macro_export]
macro_rules! print
{
	($($args:tt)+) => ({
            use core::fmt::Write;
            let uart = unsafe{ &mut UART_STRUCT };
			let _ = write!(uart, $($args)+);
	});
}
#[macro_export]
macro_rules! println
{
	() => ({
		print!("\r\n")
	});
	($fmt:expr) => ({
		print!(concat!($fmt, "\r\n"))
	});
	($fmt:expr, $($args:tt)+) => ({
		print!(concat!($fmt, "\r\n"), $($args)+)
	});
}

fn handle_irq(_irq_no: usize, arg: *mut usize) {
    let uart = unsafe { &mut UART_STRUCT };

    while let Some(c) = uart.getc() {
        xous::try_send_message(uart.rx_conn, Opcode::Char(c).into()).map(|_| ()).unwrap();
    }
}

impl Uart {
    pub fn new(connection: xous::CID) -> Uart {
        /*
           Note: this function takes over the console UART. Not compatible with console logging.
        */
        let uart = xous::syscall::map_memory(
            xous::MemoryAddress::new(utra::app_uart::HW_APP_UART_BASE),
            None,
            4096,
            xous::MemoryFlags::R | xous::MemoryFlags::W,
        )
        .expect("couldn't map debug uart");

        let uart_struct = Uart {
            uart_csr: CSR::new( uart.as_mut_ptr() as *mut u32 ),
            rx_conn: connection,
        };
        unsafe {
            UART_STRUCT.uart_csr = CSR::new( uart.as_mut_ptr() as *mut u32 );
            UART_STRUCT.rx_conn = connection;
        }

        xous::claim_interrupt(utra::app_uart::APP_UART_IRQ, handle_irq, core::ptr::null_mut::<usize>()).expect("unable to allocate IRQ");

        uart_struct
    }

    pub fn putc(&mut self, c: u8) {
        // Wait until TXFULL is `0`
        while self.uart_csr.r(utra::uart::TXFULL) != 0 {}
        self.uart_csr.wo(utra::uart::RXTX, c as u32);
    }

    pub fn enable_rx(&mut self) {
        self.uart_csr.rmwf(utra::uart::EV_ENABLE_RX, 1 );
    }

    pub fn getc(&mut self) -> Option<u8> {
        match self.uart_csr.rf(utra::uart::EV_PENDING_RX) {
            0 => None,
            ack => {
                let c = Some(self.uart_csr.rf(utra::uart::RXTX_RXTX) as u8);
                self.uart_csr.wfo(utra::uart::EV_PENDING_RX, ack);
                c
            }
        }
    }
}

impl Write for Uart {
    fn write_str(&mut self, s: &str) -> Result<(), Error> {
        for c in s.bytes() {
            self.putc(c);
        }
        Ok(())
    }
}

fn was_continuous(cmd: &mut String<U2048>) -> bool {
    let mut tokens = Vec::<_, U16>::new();
    for i in 0..16 {
        let mut empty: String<U512> = String::from("");
        tokens.push(empty).unwrap();
    }

    let mut tokindex: usize = 0;
    let mut in_space = true;
    for c in cmd.as_str().chars() {
        if in_space && (c == ' ') {
            continue;
        } else {
            if c != ' ' {
                in_space = false;
                tokens[tokindex].push(c).unwrap();
            } else {
                in_space = true;
                tokindex += 1;
                if tokindex >= 16 {
                    break;
                }
            }
        }
    }

    let command = &tokens[0];

    if command.len() == 0 {
        return false;
    }  else {
        if command.trim() == "repeat" {
            println!("repeating last PDS data");
            return true;
        }
    }
    false
}

fn do_agent(cmd: &mut String<U2048>, com_cid: xous::CID, pds_list: &mut Vec<String<U512>, U16>, info_csr: &utralib::CSR<u32>) -> Result<(), xous::Error> {
    if false {
        let tokens: Vec<&str, U16> = cmd.as_mut_str().split(' ').collect();
        for token in tokens.iter() {
            println!("token: {}", token);
        }
        return Ok(());
    }
    /*
    We want to do this:
      let tokens: Vec<&str, U16> = cmd.as_mut_str().split(' ').collect();
    But we can't. Heapless is having big problems with this for lots of reasons, so we make it manually.
    */
    let mut tokens = Vec::<_, U16>::new();
    for i in 0..16 {
        let mut empty: String<U512> = String::from("");
        tokens.push(empty).unwrap();
    }

    let mut tokindex: usize = 0;
    let mut in_space = true;
    for c in cmd.as_str().chars() {
        if in_space && (c == ' ') {
            continue;
        } else {
            if c != ' ' {
                in_space = false;
                tokens[tokindex].push(c).unwrap();
            } else {
                in_space = true;
                tokindex += 1;
                if tokindex >= 16 {
                    break;
                }
            }
        }
    }

    /*
    for token in tokens.iter() {
        println!("token: {}", token);
    }
    return Ok(());*/

    let command = &tokens[0];

    if command.len() == 0 {
        return Ok(());
    }  else {
        if command.trim() == "wfx_test_agent" {
            if tokens.len() < 2 {
                // no command specified, do nothing
                return Ok(());
            }
            if tokens[1].trim() == "read_agent_version" {
                println!("1.0.0\n\r");
            } else if tokens[1].trim() == "--help" {
                println!("I need all the help I can get.\n\r");
            } else if tokens[1].trim() == "read_fw_version" {
                let (major, minor, build) = get_wf200_fw_rev(com_cid).unwrap();
                println!("{}.{}.{}\n\r", major, minor, build);
            } else if tokens[1].trim() == "read_driver_version" {
                println!("0.0.0\n\r");
            } else if tokens[1].trim() == "write_test_data" {
                let pdsline = tokens[2].trim();
                let mut stripped: String<U512> = String::from("");
                for c in pdsline.chars() {
                    if c != '"' {
                        stripped.push(c);
                    }
                }
                pds_list.push(stripped);
            } else if tokens[1].trim() == "read_rx_stats" {
                println!("rx stats request disabled!!");
                //get_rx_stats_agent(com_cid).unwrap();
            } else if tokens[1].trim() == "commit_pds" {
                for pds in pds_list.iter() {
                    let mut sendable_string = xous::String::new(4096);
                    write!(&mut sendable_string, "{}", pds);
                    println!("{}", sendable_string);
                    send_pds_line(com_cid, &sendable_string);
                }
                pds_list.clear();
            } else if tokens[1].trim() == "ec_version" {
                let (gitrev, dirty) = get_ec_git_rev(com_cid).unwrap();
                if dirty {
                    println!("ecrev: {:08x} dirty", gitrev);
                } else {
                    println!("ecrev: {:08x} clean", gitrev);
                }
            } else if tokens[1].trim() == "soc_version" {
                if info_csr.rf(utra::info::GIT_DIRTY_DIRTY) == 1 {
                    println!("socrev: {:08x} dirty", info_csr.rf(utra::info::GIT_GITREV_GIT_GITREV));
                } else {
                    println!("socrev: {:08x} clean", info_csr.rf(utra::info::GIT_GITREV_GIT_GITREV));
                }
                println!("DNA: {:08x}{:08x}", info_csr.rf(utra::info::DNA_ID1_DNA_ID), info_csr.rf(utra::info::DNA_ID0_DNA_ID));
            } else {
                println!("{}: wfx_test_agent sub-command not recognized.", tokens[1].trim());
            }
        } else {
            println!("{}: command not recognized.", command.trim());
        }
    }
    Ok(())
}

// pull that CFFI straight through several layers of abstractions :-/
#[doc = " @brief RX stats from the GENERIC indication message sl_wfx_generic_ind_body_t"]
#[repr(C, packed)]
#[derive(Copy, Clone)]
pub struct sl_wfx_rx_stats_s {
    #[doc = "<Total number of frame received"]
    pub nb_rx_frame: u32,
    #[doc = "<Number of frame received with bad CRC"]
    pub nb_crc_frame: u32,
    #[doc = "<PER on the total number of frame"]
    pub per_total: u32,
    #[doc = "<Throughput calculated on correct frames received"]
    pub throughput: u32,
    #[doc = "<Number of frame received by rate"]
    pub nb_rx_by_rate: [u32; 22usize],
    #[doc = "<PER*10000 by frame rate"]
    pub per: [u16; 22usize],
    #[doc = "<SNR in Db*100 by frame rate"]
    pub snr: [i16; 22usize],
    #[doc = "<RSSI in Dbm*100 by frame rate"]
    pub rssi: [i16; 22usize],
    #[doc = "<CFO in k_hz by frame rate"]
    pub cfo: [i16; 22usize],
    #[doc = "<This message transmission date in firmware timebase (microsecond)"]
    pub date: u32,
    #[doc = "<Frequency of the low power clock in Hz"]
    pub pwr_clk_freq: u32,
    #[doc = "<Indicate if the low power clock is external"]
    pub is_ext_pwr_clk: u8,
}
pub type sl_wfx_rx_stats_t = sl_wfx_rx_stats_s;
#[repr(C, packed)]
#[derive(Copy, Clone)]
pub union sl_wfx_indication_data_u {
    pub rx_stats: sl_wfx_rx_stats_t,
    pub raw_data: [u8; 376],
    _bindgen_union_align: [u8; 376],
}

#[xous::xous_main]
fn xmain() -> ! {
    log_server::init_wait().unwrap();
    print!("FCCAGENT: my PID is {}", xous::process::id());

    let ticktimer_server_id = xous::SID::from_bytes(b"ticktimer-server").unwrap();
    let ticktimer_conn = xous::connect(ticktimer_server_id).unwrap();

    let agent_server_sid = xous_names::register_name(xous::names::SERVER_NAME_FCCAGENT).expect("FCCAGENT: can't register server");
    let agent_server_client = xous_names::request_connection_blocking(xous::names::SERVER_NAME_FCCAGENT).expect("FCCAGENT: can't connect to COM");

    let mut uart = Uart::new(agent_server_client);

    let com_conn = xous_names::request_connection_blocking(xous::names::SERVER_NAME_COM).expect("FCCAGENT: can't connect to COM");

    let mut cmd_string: String<U2048> = String::from("");

    let info_mem = xous::syscall::map_memory(
        xous::MemoryAddress::new(utra::info::HW_INFO_BASE),
        None,
        4096,
        xous::MemoryFlags::R | xous::MemoryFlags::W,
    )
    .expect("couldn't map INFO CSR range");
    let info_csr = CSR::new(info_mem.as_mut_ptr() as *mut u32);

    uart.enable_rx();

    print!("\n\r\n\r*** FCC agent ***\n\r\n\r");
    let mut last_time: u64 = ticktimer_server::elapsed_ms(ticktimer_conn).unwrap();
    let mut pds_list: Vec<String<U512>, U16> = Vec::new();
    let mut repeat = false;
    loop {
        let envelope = xous::try_receive_message(agent_server_sid).unwrap();
        match envelope {
            Some(env) =>  {
                if let Ok(opcode) = Opcode::try_from(&env.body) {
                    match opcode {
                        Opcode::Char(c) => {
                            if c != b'\r' && c != b'\n' {
                                print!("{}", c as char);
                                cmd_string.push(c as char).unwrap();
                            } else {
                                println!("");
                                do_agent(&mut cmd_string, com_conn, &mut pds_list, &info_csr).unwrap();
                                repeat = was_continuous(&mut cmd_string);
                                // print!("agent@precursor:~$ ");
                                cmd_string.clear();
                            }
                        },
                        Opcode::RxStats(stats) => {
                            println!("got rxstats message");
                            let mut stats_u: sl_wfx_indication_data_u = sl_wfx_indication_data_u {raw_data: [0; 376]};
                            for i in 0..stats.len() {
                               unsafe{ stats_u.raw_data[i] = stats[i]; }
                            }
                            println!("Total frames received: {}", unsafe{stats_u.rx_stats.nb_rx_frame});
                            println!("Total frames with bad CRC: {}", unsafe{stats_u.rx_stats.nb_crc_frame});
                            println!("PER on total number of frames: {}", unsafe{stats_u.rx_stats.per_total});
                            println!("Throughput on correct frames received: {}", unsafe{stats_u.rx_stats.throughput});
                            // TODO: fill in more stats output
                        },
                        _ => println!("Unknown opcode received.")
                    }
                }
            }
            _ => () //xous::yield_slice(), // no message received, idle
        }
        // Periodic tasks
        if let Ok(elapsed_time) = ticktimer_server::elapsed_ms(ticktimer_conn) {
            if elapsed_time - last_time > 20_000 {
                last_time = elapsed_time;

                if repeat {
                    for pds in pds_list.iter() {
                        let mut sendable_string = xous::String::new(4096);
                        write!(&mut sendable_string, "{}", pds);
                        println!("{}", sendable_string);
                        send_pds_line(com_conn, &sendable_string);
                    }
                }
                /*
                let mut string_buffer = xous::String::new(4096);
                write!(&mut string_buffer, "\"{{i:{{a:7,b:1,f:3E8,c:{{a:0,b:0,c:0,d:44}},d:{{a:BB8,b:0,c:0,d:15,e:64,f:4}},e:{{}}}}}}\"").expect("Can't write");
                println!("sending line: {}", string_buffer);
                send_pds_line(com_conn, &string_buffer);*/
            }
        } else {
            error!("error requesting ticktimer!")
        }
    }
}
