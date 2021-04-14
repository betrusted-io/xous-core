#[derive(Debug, Eq, PartialEq, Copy, Clone)]
pub enum Rate {
    B1Mbps,
    B2Mbps,
    B5_5Mbps,
    B11Mbps,
    G6Mbps,
    G9Mbps,
    G12Mbps,
    G18Mbps,
    G24Mbps,
    G36Mbps,
    G48Mbps,
    G54Mbps,
    NMCS0,
    NMCS1,
    NMCS2,
    NMCS3,
    NMCS4,
    NMCS5,
    NMCS6,
    NMCS7,
}

pub struct PdsRecord {
    pub rate: Rate,
    pub pds_data: &'static [[&'static str; 4]; 2],
}

pub const PDS_DATA: [PdsRecord; 2] = [
    PdsRecord {
        rate: Rate::B1Mbps,
        pds_data: &[
            ["{j:{a:0}}", "{i:{a:1,b:1,f:2255100,c:{a:0,b:0,c:0,d:44},d:{a:FFB,b:0,c:1,d:E,e:0,f:4},e:{}}}", "", ""], // channel 6
            ["{j:{a:0}}", "{i:{a:1,b:1,f:2255100,c:{a:0,b:0,c:0,d:44},d:{a:FFB,b:0,c:1,d:E,e:0,f:4},e:{}}}", "", ""], // channel 6
        ],
    },
    PdsRecord {
        rate: Rate::B2Mbps,
        pds_data: &[
            ["{j:{a:0}}", "{i:{a:1,b:1,f:2255100,c:{a:0,b:0,c:0,d:44},d:{a:FFB,b:0,c:1,d:E,e:0,f:4},e:{}}}", "", ""], // channel 6
            ["{j:{a:0}}", "{i:{a:1,b:1,f:2255100,c:{a:0,b:0,c:0,d:44},d:{a:FFB,b:0,c:1,d:E,e:0,f:4},e:{}}}", "", ""], // channel 6
        ],
    },
];

pub const PDS_STOP_DATA: &str = "{i:{a:1,b:1,f:2255100,c:{a:0,b:0,c:0,d:44},d:{a:FFB,b:0,c:1,d:E,e:64,f:4},e:{}}}";

