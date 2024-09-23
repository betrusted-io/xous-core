use String;

use crate::{CommonEnv, ShellCmdApi};

#[derive(Debug)]
pub struct TrngCmd {}
impl TrngCmd {
    pub fn new() -> Self { TrngCmd {} }
}

impl<'a> ShellCmdApi<'a> for TrngCmd {
    cmd_api!(trng);

    fn process(&mut self, args: String, env: &mut CommonEnv) -> Result<Option<String>, xous::Error> {
        use core::fmt::Write;
        let mut ret = String::new();
        let helpstring = "trng [avnist] [ronist] [runs] [excur] [errs] [pump]";

        let mut tokens = args.split(' ');

        if let Some(sub_cmd) = tokens.next() {
            match sub_cmd {
                "avnist" => {
                    let ht = env.trng.get_health_tests().unwrap();
                    write!(ret, "AV NIST stats: {:?}", ht.av_nist).unwrap();
                }
                "ronist" => {
                    let ht = env.trng.get_health_tests().unwrap();
                    write!(ret, "RO NIST stats: {:?}", ht.ro_nist).unwrap();
                }
                "runs" => {
                    let ht = env.trng.get_health_tests().unwrap();
                    for core in 0..4 {
                        write!(ret, "RO {}: ", core).unwrap();
                        for bin in 0..4 {
                            write!(ret, "{} ", ht.ro_miniruns[core].run_count[bin]).unwrap();
                        }
                        write!(ret, "\n").unwrap();
                    }
                }
                "excur" => {
                    let ht = env.trng.get_health_tests().unwrap();
                    write!(
                        ret,
                        "AV0: {}/{} mV\n",
                        ((ht.av_excursion[0].min as u32 * 1000) / 4096),
                        ((ht.av_excursion[0].max as u32 * 1000) / 4096)
                    )
                    .unwrap();
                    write!(
                        ret,
                        "AV0 delta: {} mV\n",
                        (((ht.av_excursion[0].max as u32 - ht.av_excursion[0].min as u32) * 1000) / 4096)
                    )
                    .unwrap();
                    write!(
                        ret,
                        "AV1: {}/{} mV\n",
                        ((ht.av_excursion[1].min as u32 * 1000) / 4096),
                        ((ht.av_excursion[1].max as u32 * 1000) / 4096)
                    )
                    .unwrap();
                    write!(
                        ret,
                        "AV1 delta: {} mV\n",
                        (((ht.av_excursion[1].max as u32 - ht.av_excursion[1].min as u32) * 1000) / 4096)
                    )
                    .unwrap();
                }
                "pump" => {
                    const ROUNDS: usize = 16;
                    for i in 0..ROUNDS {
                        log::info!("pump round {}", i);
                        let mut buf: [u32; 1020] = [0; 1020];
                        env.trng.fill_buf(&mut buf).unwrap();
                        log::info!("pump samples: {:x}, {:x}, {:x}", buf[0], buf[512], buf[1019]);
                    }
                    write!(ret, "Pumped {}x1k values out of the engine", ROUNDS).unwrap();
                }
                // rand_core API tests - not included by default because it only needs to be run whenever
                // the rand API is updated
                #[cfg(feature = "rand-api")]
                "api" => {
                    // the purpose of these tests is to check the edge-case code because
                    // the trng fetch is u32, but rand_core allows u8
                    use rand::RngCore;
                    use rand::rngs::OsRng;
                    let mut test1 = [0u8; 1];
                    OsRng.fill_bytes(&mut test1);
                    log::info!("test 1: {:?}", test1);

                    let mut test2 = [0u8; 2];
                    OsRng.fill_bytes(&mut test2);
                    log::info!("test 2: {:?}", test2);

                    let mut test3 = [0u8; 3];
                    OsRng.fill_bytes(&mut test3);
                    log::info!("test 3: {:?}", test3);

                    let mut test4 = [0u8; 4];
                    OsRng.fill_bytes(&mut test4);
                    log::info!("test 4: {:?}", test4);

                    let mut test5 = [0u8; 5];
                    OsRng.fill_bytes(&mut test5);
                    log::info!("test 5: {:?}", test5);

                    let mut test6 = [0u8; 6];
                    OsRng.fill_bytes(&mut test6);
                    log::info!("test 6: {:?}", test6);

                    let mut test7 = [0u8; 7];
                    OsRng.fill_bytes(&mut test7);
                    log::info!("test 7: {:?}", test7);

                    let mut test8 = [0u8; 8];
                    OsRng.fill_bytes(&mut test8);
                    log::info!("test 8: {:?}", test8);

                    let mut test4095 = [0u8; 4095];
                    OsRng.fill_bytes(&mut test4095);
                    log::info!("test 4095: {:?}", &test4095[4092..]);

                    let mut test4096 = [0u8; 4096];
                    OsRng.fill_bytes(&mut test4096);
                    log::info!("test 4096: {:?}", &test4096[4092..]);

                    let mut test4097 = [0u8; 4097];
                    OsRng.fill_bytes(&mut test4097);
                    log::info!("test 4097: {:?}", &test4097[4092..]);

                    let mut test4098 = [0u8; 4098];
                    OsRng.fill_bytes(&mut test4098);
                    log::info!("test 4098: {:?}", &test4098[4092..]);

                    let mut test4099 = [0u8; 4099];
                    OsRng.fill_bytes(&mut test4099);
                    log::info!("test 4099: {:?}", &test4099[4092..]);

                    let mut test4100 = [0u8; 4100];
                    OsRng.fill_bytes(&mut test4100);
                    log::info!("test 4100: {:?}", &test4100[4092..]);
                }
                "errs" => {
                    write!(ret, "TRNG error stats: {:?}", env.trng.get_error_stats().unwrap()).unwrap();
                }
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
