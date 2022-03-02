#[xous::xous_main]
fn test_main() -> ! {
    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Info);
    log::info!("my PID is {}", xous::process::id());

    let tt = ticktimer_server::Ticktimer::new().unwrap();
    log::info!("waiting for others to boot");
    tt.sleep_ms(5000).unwrap();
    let host = "bunniefoo.com";
    log::info!("attempting to resolve {}", host);

    let xns = xous_names::XousNames::new().unwrap();
    let dns = dns::Dns::new(&xns).unwrap();
    match dns.lookup(host) {
        Ok(ipaddr) => {
            log::info!("resolved {} to {:?}", host, ipaddr);
        }
        _ => {
            log::info!("couldn't resolve {}", host);
        }
    }

    xous::terminate_process(0)
}