use crate::{CommonEnv, ShellCmdApi};
use core::fmt::Write;
use xous_ipc::String;

#[derive(Debug)]
pub struct Ip {}

/**
ip shell command:
- list: show ip address, netmask, gateway, dns server, etc.
- ping host: send 3 ICMP pings to host (ip/hostname)
- udptx host port ...: send a UDP packet with string ... to host:port
- udprx: listen on udp localhost:port and echo payload of first incoming
  packet (5s timeout)
*/
impl<'a> ShellCmdApi<'a> for Ip {
    cmd_api!(ip); // inserts boilerplate for command API

    fn process(
        &mut self,
        args: String<1024>,
        _env: &mut CommonEnv,
    ) -> Result<Option<String<1024>>, xous::Error> {
        let mut ret = String::<1024>::new();
        let helpstring = "ip [list] [ping host] [udptx host port ...] [udprx port]";
        let mut show_help = false;

        let mut tokens = args.as_str().unwrap().split(' ');
        if let Some(sub_cmd) = tokens.next() {
            match sub_cmd {
                "list" => {
                    //env.com.ip_list().unwrap();
                    write!(ret, "ip list").unwrap();
                }
                "ping" => {
                    if let Some(host) = tokens.next() {
                        //env.com.ip_ping(&host).unwrap();
                        write!(ret, "ip ping {}", host).unwrap();
                    } else {
                        show_help = true;
                    }
                }
                "udptx" => {
                    if let (Some(host), Some(port)) = (tokens.next(), tokens.next()) {
                        let mut val = String::<1024>::new();
                        join_tokens(&mut val, &mut tokens);
                        //env.com.ip_udptx(&host, &port, &val).unwrap();
                        write!(ret, "ip udptx {} {} {}", host, port, val).unwrap();
                    } else {
                        show_help = true;
                    }
                }
                "udprx" => {
                    if let Some(port) = tokens.next() {
                        //env.com.ip_updprx(&port).unwrap();
                        write!(ret, "ip udprx {}", port).unwrap();
                    } else {
                        show_help = true;
                    }
                }
                _ => {
                    show_help = true;
                }
            }
        } else {
            show_help = true;
        }
        if show_help {
            write!(ret, "{}", helpstring).unwrap();
        }
        Ok(Some(ret))
    }
}

/**
Join an iterator of string tokens with spaces.

This is intended to reverse the effect of .split(' ') in the context of a very simple
command parser. This is a lazy way to avoid building a parser for quoted strings, since
SSIDs or passwords might include spaces.
*/
fn join_tokens<'a>(buf: &mut String<1024>, tokens: impl Iterator<Item = &'a str>) {
    for (i, tok) in tokens.enumerate() {
        if i == 0 {
            write!(buf, "{}", tok).unwrap();
        } else {
            write!(buf, " {}", tok).unwrap();
        }
    }
}
