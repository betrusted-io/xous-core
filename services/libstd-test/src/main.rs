#[xous::xous_main]
fn test_main() -> ! {
    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Info);
    log::info!("my PID is {}", xous::process::id());

    let tt = ticktimer_server::Ticktimer::new().unwrap();
    log::info!("waiting for others to boot");
    tt.sleep_ms(1000).unwrap();
    let xns = xous_names::XousNames::new().unwrap();
    let mut com = com::Com::new(&xns).unwrap();
    com.wlan_join().expect("couldn't issue join command");
    log::info!("waiting for join to finish");
    tt.sleep_ms(1000).unwrap();

    let host = "bunniefoo.com";
    log::info!("attempting to resolve {}", host);

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