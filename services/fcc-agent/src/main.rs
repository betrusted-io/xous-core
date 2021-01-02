#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

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

    let agent_server_sid = xous::create_server_with_address(b"fcc-agent-server").expect("Couldn't create FCC Agent server");
    let agent_server_client = xous::connect(xous::SID::from_bytes(b"fcc-agent-server").unwrap()).expect("couldn't connect to self");

    let mut uart = Uart::new(agent_server_client);

    let ticktimer_server_id = xous::SID::from_bytes(b"ticktimer-server").unwrap();
    let ticktimer_conn = xous::connect(ticktimer_server_id).unwrap();

    let com_id = xous::SID::from_bytes(b"com             ").unwrap();
    let com_conn = xous::connect(com_id).unwrap();

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
