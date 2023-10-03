use std::io::{ErrorKind, IoSlice, IoSliceMut, Read, Write};
use std::net::*;
use std::sync::mpsc::channel;
use std::thread;
use std::time::{Duration, Instant};

use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::sync::atomic::{AtomicUsize, Ordering};

pub(crate) fn start_batch_tests() {
    let _ = thread::spawn({
        move || {
            let run_passing = false;
            let tt = ticktimer_server::Ticktimer::new().unwrap();
            const PRINT_DELAY: usize = 2000;
            if run_passing {
                log::info!("################################################## bind_error");
                tt.sleep_ms(PRINT_DELAY).ok();
                bind_error();
                log::info!("################################################## connect_error");
                tt.sleep_ms(PRINT_DELAY).ok();
                connect_error();
                log::info!("################################################## listen_localhost");
                tt.sleep_ms(PRINT_DELAY).ok();
                listen_localhost();
                log::info!("################################################## connect_loopback");
                tt.sleep_ms(PRINT_DELAY).ok();
                connect_loopback();
                log::info!("################################################## smoke_test");
                tt.sleep_ms(PRINT_DELAY).ok();
                smoke_test();
                log::info!("################################################## read_eof");
                tt.sleep_ms(PRINT_DELAY).ok();
                read_eof();

                // This test can fail if there is a ticktimer scheduling error:
                /*
                    INFO:shellchat::nettests: ################################################## write_close (services\shellchat\src\nettests.rs:35)
                    INFO:shellchat::nettests: ++++++++++++++++++++++++++create server (services\shellchat\src\nettests.rs:252)
                    INFO:shellchat::nettests: ++++++++++++++++++++++++++try to establish connection (services\shellchat\src\nettests.rs:265)
                    INFO:shellchat::nettests: ++++++++++++++++++++++++++established (services\shellchat\src\nettests.rs:267)
                    INFO:shellchat::nettests: ----------------------------create and drop connection to server (services\shellchat\src\nettests.rs:257)
                    INFO:shellchat::nettests: ----------------------------signal that we should proceed to send the data to the closed server (services\shellchat\src\nettests.rs:262)
                    ERR :xous_ticktimer: requested to wake 1 entries, which is more than the current 2 waiting entries (services\xous-ticktimer\src\main.rs:429)
                       -- hangs here forever.
                 */
                log::info!("################################################## write_close");
                tt.sleep_ms(PRINT_DELAY).ok();
                write_close();
                log::info!("################################################## partial_read");
                tt.sleep_ms(PRINT_DELAY).ok();
                partial_read();

                // note: these tests don't pass in their native form because our Close is blocking
                // tried to make it non-blocking but other things break because the smoltcp layer doesn't
                // clear enough state if you just let things move on (I think).
                log::info!("################################################## read_vectored");
                tt.sleep_ms(PRINT_DELAY).ok();
                read_vectored();
                log::info!("################################################## write_vectored");
                tt.sleep_ms(PRINT_DELAY).ok();
                write_vectored();

                log::info!("################################################## nodelay");
                nodelay();
                tt.sleep_ms(PRINT_DELAY).ok();
                log::info!("################################################## ttl");
                tt.sleep_ms(PRINT_DELAY).ok();
                ttl();
                log::info!("################################################## set_nonblocking");
                tt.sleep_ms(PRINT_DELAY).ok();
                set_nonblocking();
                log::info!("################################################## peek");
                tt.sleep_ms(PRINT_DELAY).ok();
                peek();

                log::info!("################################################## timeouts");
                tt.sleep_ms(PRINT_DELAY).ok();
                timeouts();
                log::info!("################################################## test_read_timeout");
                tt.sleep_ms(PRINT_DELAY).ok();
                test_read_timeout();
                log::info!("################################################## test_read_with_timeout");
                tt.sleep_ms(PRINT_DELAY).ok();
                test_read_with_timeout();
                log::info!("################################################## test_timeout_zero_duration");
                tt.sleep_ms(PRINT_DELAY).ok();
                test_timeout_zero_duration();
                log::info!("################################################## connect_timeout_valid");
                tt.sleep_ms(PRINT_DELAY).ok();
                connect_timeout_valid();

                log::info!("################################################## tcp_clone_smoke");
                tt.sleep_ms(PRINT_DELAY).ok();
                tcp_clone_smoke();
                log::info!("################################################## tcp_clone_two_read");
                tt.sleep_ms(PRINT_DELAY).ok();
                tcp_clone_two_read();
                log::info!("################################################## tcp_clone_two_write");
                tt.sleep_ms(PRINT_DELAY).ok();
                tcp_clone_two_write();
                log::info!("################################################## clone_while_reading");
                tt.sleep_ms(PRINT_DELAY).ok();
                clone_while_reading();
            }

            log::info!("################################################## clone_accept_smoke");
            tt.sleep_ms(PRINT_DELAY).ok();
            clone_accept_smoke();
            log::info!("################################################## clone_accept_concurrent");
            tt.sleep_ms(PRINT_DELAY).ok();
            clone_accept_concurrent();

            log::info!("################################################## multiple_connect_serial");
            tt.sleep_ms(PRINT_DELAY).ok();
            multiple_connect_serial();
            log::info!("################################################## multiple_connect_interleaved_greedy_schedule");
            tt.sleep_ms(PRINT_DELAY).ok();
            multiple_connect_interleaved_greedy_schedule();
            log::info!("################################################## multiple_connect_interleaved_lazy_schedule");
            tt.sleep_ms(PRINT_DELAY).ok();
            multiple_connect_interleaved_lazy_schedule();

            log::info!("################################################## FIN");
            log::info!("################################################## FIN");
            log::info!("################################################## FIN");
            tt.sleep_ms(PRINT_DELAY).ok();
        }
    });
}


static PORT: AtomicUsize = AtomicUsize::new(0);

fn base_port() -> u16 { 19600 }

pub fn next_test_ip4() -> SocketAddr {
    let port = PORT.fetch_add(1, Ordering::SeqCst) as u16 + base_port();
    SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(127, 0, 0, 1), port))
}

fn each_ip(f: &mut dyn FnMut(SocketAddr)) {
    f(next_test_ip4());
}

macro_rules! t {
    ($e:expr) => {
        match $e {
            Ok(t) => t,
            Err(e) => panic!("received error for `{}`: {}", stringify!($e), e),
        }
    };
}

fn bind_error() {
    match TcpListener::bind("1.1.1.1:9999") {
        Ok(..) => panic!(),
        Err(e) =>
        {
            assert_eq!(e.kind(), ErrorKind::AddrNotAvailable); // this will break on earlier versions of stdlib
        }
    }
}

fn connect_error() {
    match TcpStream::connect("0.0.0.0:1") {
        Ok(..) => panic!(),
        Err(e) => {
            log::info!("connect_error is {:?}", e);
            assert!(
            e.kind() == ErrorKind::ConnectionRefused
                || e.kind() == ErrorKind::InvalidInput // this is how we will report it in 1.62.1
                || e.kind() == ErrorKind::AddrInUse
                || e.kind() == ErrorKind::AddrNotAvailable, // this is how it's reported on Windows
            "bad error: {} {:?}",
            e,
            e.kind()
        )},
    }
}

fn listen_localhost() {
    let socket_addr = next_test_ip4();
    //log::info!("~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~listen_localhost with addr: {:?}", socket_addr);
    let listener = t!(TcpListener::bind(&socket_addr));
    //log::info!("~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~Listener created");

    let _t = thread::spawn(move || {
        //log::info!("+++++++++++++++++++++++++++++++++in Tx thread");
        let mut stream = t!(TcpStream::connect(&("localhost", socket_addr.port())));
        //log::info!("+++++++++++++++++++++++++++++++++spawned localhost writer");
        t!(stream.write(&[144]));
        //log::info!("+++++++++++++++++++++++++++++++++wrote test byte");
    });

    //log::info!("~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~calling listener.accept()");
    let mut stream = t!(listener.accept()).0;
    let mut buf = [0];
    //log::info!("~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~got localhost stream");
    t!(stream.read(&mut buf));
    assert!(buf[0] == 144);
    //log::info!("~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~got test byte");
}

fn connect_loopback() {
    each_ip(&mut |addr| {
        let acceptor = t!(TcpListener::bind(&addr));

        let _t = thread::spawn(move || {
            let host = match addr {
                SocketAddr::V4(..) => "127.0.0.1",
                SocketAddr::V6(..) => "::1",
            };
            let mut stream = t!(TcpStream::connect(&(host, addr.port())));
            t!(stream.write(&[66]));
        });

        let mut stream = t!(acceptor.accept()).0;
        let mut buf = [0];
        t!(stream.read(&mut buf));
        assert!(buf[0] == 66);
    })
}

fn smoke_test() {
    each_ip(&mut |addr| {
        let acceptor = t!(TcpListener::bind(&addr));

        let (tx, rx) = channel();
        let _t = thread::spawn(move || {
            let mut stream = t!(TcpStream::connect(&addr));
            t!(stream.write(&[99]));
            tx.send(t!(stream.local_addr())).unwrap();
        });

        let (mut stream, addr) = t!(acceptor.accept());
        let mut buf = [0];
        t!(stream.read(&mut buf));
        assert!(buf[0] == 99);
        assert_eq!(addr, t!(rx.recv()));
    })
}

fn read_eof() {
    each_ip(&mut |addr| {
        let acceptor = t!(TcpListener::bind(&addr));

        let _t = thread::spawn(move || {
            let _stream = t!(TcpStream::connect(&addr));
            // Close
        });

        let mut stream = t!(acceptor.accept()).0;
        let mut buf = [0];
        let nread = t!(stream.read(&mut buf));
        assert_eq!(nread, 0);
        let nread = t!(stream.read(&mut buf));
        assert_eq!(nread, 0);
    })
}

fn write_close() {
    each_ip(&mut |addr| {
        let acceptor = t!(TcpListener::bind(&addr)); // listener is on 127.0.0.1
        log::info!("++++++++++++++++++++++++++create server");

        let (tx, rx) = channel();
        let _t = thread::spawn(move || {
            drop(t!(TcpStream::connect(&addr))); // this comes from 10.0.245.184
            log::info!("----------------------------create and drop connection to server");
            // we get stuck on the above connect trying to close its connection to the TcpListener
            // it is stuck on FinWait2
            // the Listener is not responding with an ACK
            tx.send(()).unwrap();
            log::info!("----------------------------signal that we should proceed to send the data to the closed server");
        });

        log::info!("++++++++++++++++++++++++++try to establish connection");
        let mut stream = t!(acceptor.accept()).0;
        log::info!("++++++++++++++++++++++++++established");
        // the stuck-ness happens around here in real terms
        rx.recv().unwrap();
        let buf = [0];
        log::info!("++++++++++++++++++++++++++try to write to a stream that's now closed");
        match stream.write(&buf) {
            Ok(..) => {}
            Err(e) => {
                assert!(
                    e.kind() == ErrorKind::ConnectionReset
                        || e.kind() == ErrorKind::BrokenPipe
                        || e.kind() == ErrorKind::ConnectionAborted,
                    "unknown error: {}", e
                );
            }
        }
        log::info!("++++++++++++++++++++++++++done");
    })
}

fn multiple_connect_serial() {
    each_ip(&mut |addr| {
        let max = 3;
        let acceptor = t!(TcpListener::bind(&addr));

        let _t = thread::spawn(move || {
            for _ in 0..max {
                let mut stream = t!(TcpStream::connect(&addr));
                t!(stream.write(&[99]));
            }
        });

        for stream in acceptor.incoming().take(max) {
            let mut stream = t!(stream);
            let mut buf = [0];
            t!(stream.read(&mut buf));
            assert_eq!(buf[0], 99);
        }
    })
}

fn multiple_connect_interleaved_greedy_schedule() {
    const MAX: usize = 3;
    each_ip(&mut |addr| {
        let acceptor = t!(TcpListener::bind(&addr));

        let _t = thread::spawn(move || {
            let acceptor = acceptor;
            for (i, stream) in acceptor.incoming().enumerate().take(MAX) {
                // Start another thread to handle the connection
                let _t = thread::spawn(move || {
                    let mut stream = t!(stream);
                    let mut buf = [0];
                    t!(stream.read(&mut buf));
                    assert!(buf[0] == i as u8);
                });
            }
        });

        connect(0, addr);
    });

    fn connect(i: usize, addr: SocketAddr) {
        if i == MAX {
            return;
        }

        let t = thread::spawn(move || {
            let mut stream = t!(TcpStream::connect(&addr));
            // Connect again before writing
            connect(i + 1, addr);
            t!(stream.write(&[i as u8]));
        });
        t.join().ok().expect("thread panicked");
    }
}


fn multiple_connect_interleaved_lazy_schedule() {
    const MAX: usize = 3;
    each_ip(&mut |addr| {
        let acceptor = t!(TcpListener::bind(&addr));

        let _t = thread::spawn(move || {
            for stream in acceptor.incoming().take(MAX) {
                // Start another thread to handle the connection
                let _t = thread::spawn(move || {
                    let mut stream = t!(stream);
                    let mut buf = [0];
                    t!(stream.read(&mut buf));
                    assert!(buf[0] == 99);
                });
            }
        });

        connect(0, addr);
    });

    fn connect(i: usize, addr: SocketAddr) {
        if i == MAX {
            return;
        }

        let t = thread::spawn(move || {
            let mut stream = t!(TcpStream::connect(&addr));
            connect(i + 1, addr);
            t!(stream.write(&[99]));
        });
        t.join().ok().expect("thread panicked");
    }
}


fn partial_read() {
    each_ip(&mut |addr| {
        let (tx, rx) = channel();
        let srv = t!(TcpListener::bind(&addr));
        let _t = thread::spawn(move || {
            let mut cl = t!(srv.accept()).0;
            cl.write(&[10]).unwrap();
            let mut b = [0];
            t!(cl.read(&mut b));
            tx.send(()).unwrap();
        });

        let mut c = t!(TcpStream::connect(&addr));
        let mut b = [0; 10];
        assert_eq!(c.read(&mut b).unwrap(), 1);
        t!(c.write(&[1]));
        rx.recv().unwrap();
    })
}

fn read_vectored() {
    each_ip(&mut |addr| {
        let srv = t!(TcpListener::bind(&addr));
        let mut s1 = t!(TcpStream::connect(&addr));
        let mut s2 = t!(srv.accept()).0;

        let _ = thread::spawn(move || {
            let len = s1.write(&[10, 11, 12]).unwrap();
            assert_eq!(len, 3);
        });

        let mut a = [];
        let mut b = [0];
        let mut c = [0; 3];
        let len = t!(s2.read_vectored(&mut [
            IoSliceMut::new(&mut a),
            IoSliceMut::new(&mut b),
            IoSliceMut::new(&mut c)
        ],));
        assert!(len > 0);
        assert_eq!(b, [10]);
        log::info!("len: {}, c: {:?}", len, c);
        // some implementations don't support readv, so we may only fill the first buffer
        assert!(len == 1 || c == [11, 12, 0]);
    })
}

fn write_vectored() {
    each_ip(&mut |addr| {
        let srv = t!(TcpListener::bind(&addr));
        let mut s1 = t!(TcpStream::connect(&addr));
        let mut s2 = t!(srv.accept()).0;

        let _ = thread::spawn(move || {
            let a = [];
            let b = [10];
            let c = [11, 12];
            t!(s1.write_vectored(&[IoSlice::new(&a), IoSlice::new(&b), IoSlice::new(&c)]));
        });

        let mut buf = [0; 4];
        let len = t!(s2.read(&mut buf));
        // some implementations don't support writev, so we may only write the first buffer
        if len == 1 {
            assert_eq!(buf, [10, 0, 0, 0]);
        } else {
            assert_eq!(len, 3);
            assert_eq!(buf, [10, 11, 12, 0]);
        }
    })
}


fn tcp_clone_smoke() {
    each_ip(&mut |addr| {
        let acceptor = t!(TcpListener::bind(&addr));

        let _t = thread::spawn(move || {
            let mut s = t!(TcpStream::connect(&addr));
            let mut buf = [0, 0];
            assert_eq!(s.read(&mut buf).unwrap(), 1);
            assert_eq!(buf[0], 1);
            t!(s.write(&[2]));
        });

        let mut s1 = t!(acceptor.accept()).0;
        let s2 = t!(s1.try_clone());

        let (tx1, rx1) = channel();
        let (tx2, rx2) = channel();
        let _t = thread::spawn(move || {
            let mut s2 = s2;
            rx1.recv().unwrap();
            t!(s2.write(&[1]));
            tx2.send(()).unwrap();
        });
        tx1.send(()).unwrap();
        let mut buf = [0, 0];
        assert_eq!(s1.read(&mut buf).unwrap(), 1);
        rx2.recv().unwrap();
    })
}

fn tcp_clone_two_read() {
    each_ip(&mut |addr| {
        let acceptor = t!(TcpListener::bind(&addr));
        let (tx1, rx) = channel();
        let tx2 = tx1.clone();

        let _t = thread::spawn(move || {
            let mut s = t!(TcpStream::connect(&addr));
            t!(s.write(&[1]));
            rx.recv().unwrap();
            t!(s.write(&[2]));
            rx.recv().unwrap();
        });

        let mut s1 = t!(acceptor.accept()).0;
        let s2 = t!(s1.try_clone());

        let (done, rx) = channel();
        let _t = thread::spawn(move || {
            let mut s2 = s2;
            let mut buf = [0, 0];
            t!(s2.read(&mut buf));
            tx2.send(()).unwrap();
            done.send(()).unwrap();
        });
        let mut buf = [0, 0];
        t!(s1.read(&mut buf));
        tx1.send(()).unwrap();

        rx.recv().unwrap();
    })
}

fn tcp_clone_two_write() {
    each_ip(&mut |addr| {
        let acceptor = t!(TcpListener::bind(&addr));

        let _t = thread::spawn(move || {
            let mut s = t!(TcpStream::connect(&addr));
            let mut buf = [0, 1];
            t!(s.read(&mut buf));
            t!(s.read(&mut buf));
        });

        let mut s1 = t!(acceptor.accept()).0;
        let s2 = t!(s1.try_clone());

        let (done, rx) = channel();
        let _t = thread::spawn(move || {
            let mut s2 = s2;
            t!(s2.write(&[1]));
            done.send(()).unwrap();
        });
        t!(s1.write(&[2]));

        rx.recv().unwrap();
    })
}

fn clone_while_reading() {
    each_ip(&mut |addr| {
        let accept = t!(TcpListener::bind(&addr));

        // Enqueue a thread to write to a socket
        let (tx, rx) = channel();
        let (txdone, rxdone) = channel();
        let txdone2 = txdone.clone();
        let _t = thread::spawn(move || {
            let mut tcp = t!(TcpStream::connect(&addr));
            rx.recv().unwrap();
            t!(tcp.write(&[0]));
            txdone2.send(()).unwrap();
        });

        // Spawn off a reading clone
        let tcp = t!(accept.accept()).0;
        let tcp2 = t!(tcp.try_clone());
        let txdone3 = txdone.clone();
        let _t = thread::spawn(move || {
            let mut tcp2 = tcp2;
            t!(tcp2.read(&mut [0]));
            txdone3.send(()).unwrap();
        });

        // Try to ensure that the reading clone is indeed reading
        for _ in 0..50 {
            thread::yield_now();
        }

        // clone the handle again while it's reading, then let it finish the
        // read.
        let _ = t!(tcp.try_clone());
        tx.send(()).unwrap();
        rxdone.recv().unwrap();
        rxdone.recv().unwrap();
    })
}

fn clone_accept_smoke() {
    each_ip(&mut |addr| {
        let a = t!(TcpListener::bind(&addr));
        let a2 = t!(a.try_clone());

        let _t = thread::spawn(move || {
            let _ = TcpStream::connect(&addr);
        });
        let _t = thread::spawn(move || {
            let _ = TcpStream::connect(&addr);
        });

        t!(a.accept());
        t!(a2.accept());
    })
}

fn clone_accept_concurrent() {
    each_ip(&mut |addr| {
        let a = t!(TcpListener::bind(&addr));
        let a2 = t!(a.try_clone());

        let (tx, rx) = channel();
        let tx2 = tx.clone();

        let _t = thread::spawn(move || {
            tx.send(t!(a.accept())).unwrap();
        });
        let _t = thread::spawn(move || {
            tx2.send(t!(a2.accept())).unwrap();
        });

        let _t = thread::spawn(move || {
            let _ = TcpStream::connect(&addr);
        });
        let _t = thread::spawn(move || {
            let _ = TcpStream::connect(&addr);
        });

        rx.recv().unwrap();
        rx.recv().unwrap();
    })
}


// FIXME: re-enabled openbsd tests once their socket timeout code
//        no longer has rounding errors.
// VxWorks ignores SO_SNDTIMEO.
fn timeouts() {
    let addr = next_test_ip4();
    log::info!("timeout to addr {:?}", addr);
    let listener = t!(TcpListener::bind(&addr));

    let handle = thread::spawn(move || {
        log::info!("making stream");
        let stream = t!(TcpStream::connect(&("localhost", addr.port())));
        let dur = Duration::new(15410, 0);

        log::info!("checking null read timeout");
        assert_eq!(None, t!(stream.read_timeout()));

        log::info!("setting and reading read timeout");
        t!(stream.set_read_timeout(Some(dur)));
        assert_eq!(Some(dur), t!(stream.read_timeout()));

        log::info!("checking null write timeout");
        assert_eq!(None, t!(stream.write_timeout()));

        log::info!("setting and reading write timeout");
        t!(stream.set_write_timeout(Some(dur)));
        assert_eq!(Some(dur), t!(stream.write_timeout()));

        log::info!("resetting timeouts to zero");
        t!(stream.set_read_timeout(None));
        assert_eq!(None, t!(stream.read_timeout()));

        t!(stream.set_write_timeout(None));
        assert_eq!(None, t!(stream.write_timeout()));
        log::info!("closing connection");
    });
    let _ = handle.join();
    drop(listener);
    log::info!("closed");
}

fn test_read_timeout() {
    let addr = next_test_ip4();
    let listener = t!(TcpListener::bind(&addr));

    let mut stream = t!(TcpStream::connect(&("localhost", addr.port())));
    t!(stream.set_read_timeout(Some(Duration::from_millis(1000))));

    let mut buf = [0; 10];
    let start = Instant::now();
    let kind = stream.read_exact(&mut buf).err().expect("expected error").kind();
    assert!(
        kind == ErrorKind::WouldBlock || kind == ErrorKind::TimedOut,
        "unexpected_error: {:?}",
        kind
    );
    assert!(start.elapsed() > Duration::from_millis(400));
    drop(listener);
}

fn test_read_with_timeout() {
    let addr = next_test_ip4();
    let listener = t!(TcpListener::bind(&addr));

    let mut stream = t!(TcpStream::connect(&("localhost", addr.port())));
    t!(stream.set_read_timeout(Some(Duration::from_millis(1000))));

    let mut other_end = t!(listener.accept()).0;
    t!(other_end.write_all(b"hello world"));

    let mut buf = [0; 11];
    t!(stream.read(&mut buf));
    assert_eq!(b"hello world", &buf[..]);

    let start = Instant::now();
    let kind = stream.read_exact(&mut buf).err().expect("expected error").kind();
    assert!(
        kind == ErrorKind::WouldBlock || kind == ErrorKind::TimedOut,
        "unexpected_error: {:?}",
        kind
    );
    assert!(start.elapsed() > Duration::from_millis(400));
    drop(listener);
}

// Ensure the `set_read_timeout` and `set_write_timeout` calls return errors
// when passed zero Durations
fn test_timeout_zero_duration() {
    let addr = next_test_ip4();

    let listener = t!(TcpListener::bind(&addr));
    let stream = t!(TcpStream::connect(&addr));

    let result = stream.set_write_timeout(Some(Duration::new(0, 0)));
    let err = result.unwrap_err();
    assert_eq!(err.kind(), ErrorKind::InvalidInput);

    let result = stream.set_read_timeout(Some(Duration::new(0, 0)));
    let err = result.unwrap_err();
    assert_eq!(err.kind(), ErrorKind::InvalidInput);

    drop(listener);
}

#[allow(dead_code)] // until issue #210 is resolved
fn nodelay() {
    let addr = next_test_ip4();
    log::info!("starting listener");
    let listener = t!(TcpListener::bind(&addr));

    let handle = thread::spawn(move || {
        log::info!("connect to localhost port {}", addr.port());
        let stream = t!(TcpStream::connect(&("localhost", addr.port())));

        assert_eq!(false, t!(stream.nodelay()));
        t!(stream.set_nodelay(true));
        assert_eq!(true, t!(stream.nodelay()));
        t!(stream.set_nodelay(false));
        assert_eq!(false, t!(stream.nodelay()));
    });
    let _ = handle.join();
    drop(listener);
}

fn ttl() {
    let ttl = 100;

    let addr = next_test_ip4();
    let listener = t!(TcpListener::bind(&addr));

    t!(listener.set_ttl(ttl));
    assert_eq!(ttl, t!(listener.ttl()));

    let handle = thread::spawn(move || {
        let stream = t!(TcpStream::connect(&("localhost", addr.port())));

        t!(stream.set_ttl(ttl));
        assert_eq!(ttl, t!(stream.ttl()));
    });
    let _ = handle.join();
    drop(listener);
}

#[allow(dead_code)] // not yet implemented
fn set_nonblocking() {
    let addr = next_test_ip4();
    let listener = t!(TcpListener::bind(&addr));
    log::info!("setting nonblocking on listener");
    t!(listener.set_nonblocking(true));
    t!(listener.set_nonblocking(false));

    let handle = thread::spawn(move || {
        let mut stream = t!(TcpStream::connect(&("localhost", addr.port())));
        log::info!("setting nonblocking on client");
        t!(stream.set_nonblocking(false));
        t!(stream.set_nonblocking(true));

        let mut buf = [0];
        log::info!("attempting to read on a socket with no data");
        match stream.read(&mut buf) {
            Ok(_) => panic!("expected error"),
            Err(ref e) if e.kind() == ErrorKind::WouldBlock => {}
            Err(e) => panic!("unexpected error {}", e),
        }
    });
    let _ = handle.join();
    drop(listener);
}

#[cfg_attr(target_env = "sgx", ignore)] // FIXME: https://github.com/fortanix/rust-sgx/issues/31
fn peek() {
    each_ip(&mut |addr| {
        log::info!("starting mpsc");
        let (txdone, rxdone) = channel();

        log::info!("building server");
        let srv = t!(TcpListener::bind(&addr));
        let _t = thread::spawn(move || {
            log::info!("waiting for a connection");
            let mut cl = t!(srv.accept()).0;
            log::info!("filling numbers in connection");
            cl.write(&[1, 3, 3, 7]).unwrap();
            log::info!("waiting for signal to close");
            t!(rxdone.recv());
            log::info!("closing");
        });

        log::info!("connecting to server");
        let mut c = t!(TcpStream::connect(&addr));
        let mut b = [0; 10];
        for i in 1..3 {
            log::info!("peek iter {}", i);
            let len = c.peek(&mut b).unwrap();
            log::info!("peek data {:?}", b);
            assert_eq!(len, 4);
        }
        let len = c.read(&mut b).unwrap();
        log::info!("read data {:?} of len {}", b, len);
        assert_eq!(len, 4);

        log::info!("testing nonblocking peek of now empty stream");
        t!(c.set_nonblocking(true));
        match c.peek(&mut b) {
            Ok(_) => panic!("expected error"),
            Err(ref e) if e.kind() == ErrorKind::WouldBlock => {}
            Err(e) => panic!("unexpected error {}", e),
        }
        log::info!("informing server it can close");
        t!(txdone.send(()));
    })
}

#[cfg_attr(target_env = "sgx", ignore)] // FIXME: https://github.com/fortanix/rust-sgx/issues/31
fn connect_timeout_valid() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    TcpStream::connect_timeout(&addr, Duration::from_secs(2)).unwrap();
}
