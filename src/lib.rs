use bao1x_api::IoxPort;
use bytemuck::{Pod, Zeroable};
use hex_literal::hex;
use rand::Rng;
use std::io::{Read, Write};

pub const DC34_DICT: &str = "dc34";
pub const DC34_SECRET: &str = "k0";
pub const DC34_TOUR: &str = "tour";
pub const DC34_TOKEN_TOUR: &str = "tokentour";
pub const DC34_BADGE: &str = "badge";
pub const DC34_GENE: &str = "gene";
pub const DC34_IMAGE: &str = "image";
pub const DC34_BIO: &str = "bio.code";
pub const DC34_BIO_PINS: &str = "bio.pins";
pub const DC34_BIO_CLK: &str = "bio.clk";

pub const SERVER_NAME_VAULT2: &str = "_Vault2_";

/// This is the offset of the one-way counter used to gate operations that
/// can only be run in the factory: once this is greater than 0, the operations
/// are disabled. 128 is the beginning of the 'application' range for OWC.
pub const FACTORY_ONE_WAY: usize = 128;

// chosen by fair dice roll. guaranteed to be random.
pub const DC34_HEADER: [u8; 16] = hex!("49db7671 f34435ed 5fddffdf cbb7508a");

pub const SAO_GPIO: [(IoxPort, u8); 4] = [
    (IoxPort::PC, 5),
    (IoxPort::PC, 6),
    (IoxPort::PC, 14),
    (IoxPort::PC, 15),
];

pub const LED_SERVER: &'static str = "_oem_led_";
#[derive(Debug, Copy, Clone, num_derive::FromPrimitive, num_derive::ToPrimitive)]
pub enum LedManagerOp {
    Autogamy,
    Force,
    GeneTest,
    SetGene,
    Syngamy,
    JackEyes,
    Invalid,
    SetTestRate,
    // this is a hard-coded inside the bio-service to avoid leaking project to public repos
    Pause = 128,
}

pub const POWER_MANAGER_SERVER: &'static str = "_dc34_pwr_mgr_";
#[derive(Debug, Copy, Clone, num_derive::FromPrimitive, num_derive::ToPrimitive)]
pub enum PowerManagerOp {
    Enable,
    Poll,
    MotionIrq,
    VbusIrq,
    KeyPress,
    SetFadeMode,
    GetAccelId,
    GetVbat,
    GetVbus,
    Boot,
    ForceWfi,
    ForceDeepSleep,
    PowerOff,
    ScreenOffRequest,
    PauseAccel,
    FeedWdt,
    Invalid,
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum BadgeType {
    Uber = 0,
    Other = 1,
    Community = 2,
    Village = 3,
    CtfContest = 4,
    Human = 5,
    Goon = 6,
    None = 7,
}
impl TryFrom<u8> for BadgeType {
    type Error = u8;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(BadgeType::Uber),
            1 => Ok(BadgeType::Other),
            2 => Ok(BadgeType::Community),
            3 => Ok(BadgeType::Village),
            4 => Ok(BadgeType::CtfContest),
            5 => Ok(BadgeType::Human),
            6 => Ok(BadgeType::Goon),
            7 => Ok(BadgeType::None),
            n => Err(n),
        }
    }
}
impl BadgeType {
    pub fn hue_range(&self) -> std::ops::RangeInclusive<u8> {
        match self {
            Self::Goon => 0..=20,
            Self::Community => 32..=80,
            Self::Village => 80..=128,
            Self::Human => 128..=160,
            Self::Other => 160..=192,
            Self::CtfContest => 192..=220,
            Self::Uber => 220..=255,
            Self::None => 128..=160,
        }
    }
    pub fn sat_range(&self) -> std::ops::RangeInclusive<u8> {
        match self {
            Self::Goon => 160..=255,
            Self::Community => 32..=160,
            Self::Village => 32..=160,
            Self::Human => 32..=255,
            Self::Other => 16..=255,
            Self::CtfContest => 16..=255,
            Self::Uber => 130..=255,
            Self::None => 32..=255,
        }
    }
    pub fn chaser_range(&self) -> std::ops::RangeInclusive<u8> {
        match self {
            Self::Goon => 90..=255,
            Self::Community => 90..=255,
            Self::Village => 90..=255,
            Self::Human => 90..=255,
            Self::Other => 0..=255,
            Self::CtfContest => 90..=255,
            Self::Uber => 0..=45,
            Self::None => 90..=255,
        }
    }
    pub fn nonlin_range(&self) -> std::ops::RangeInclusive<u8> {
        match self {
            Self::Goon => 0..=255,
            Self::Community => 0..=255,
            Self::Village => 0..=255,
            Self::Human => 0..=255,
            Self::Other => 0..=90,
            Self::CtfContest => 0..=90,
            Self::Uber => 0..=44,
            Self::None => 0..=255,
        }
    }
    pub fn cd_dir_range(&self) -> std::ops::RangeInclusive<u8> {
        match self {
            Self::Goon => 0..=255,
            Self::Community => 0..=255,
            Self::Village => 0..=45,
            Self::Human => 0..=255,
            Self::Other => 0..=255,
            Self::CtfContest => 0..=255,
            Self::Uber => 0..=45,
            Self::None => 0..=255,
        }
    }
    pub fn cd_period_max(&self) -> u8 {
        match self {
            Self::Goon => 4,
            Self::Community => 2,
            Self::Village => 4,
            Self::Human => 5,
            Self::Other => 6,
            Self::CtfContest => 6,
            Self::Uber => 3,
            Self::None => 4,
        }
    }
}

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
#[repr(u8)]
pub enum MutationRate {
    None = 0,
    Baseline = 64,
    Elevated = 100,
    Radioactive = 140,
    Apocalyptic = 240,
}
impl PartialOrd for MutationRate {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for MutationRate {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (*self as u8).cmp(&(*other as u8))
    }
}
impl MutationRate {
    pub fn to_bit_changes(self) -> u8 {
        match self {
            MutationRate::None => 0,
            MutationRate::Baseline => 1,
            MutationRate::Elevated => 3,
            MutationRate::Radioactive => 7,
            MutationRate::Apocalyptic => 0x1f,
        }
    }
    pub fn roll(&self) -> bool {
        if *self == MutationRate::None {
            false
        } else {
            rand::thread_rng().gen::<u8>() < *self as u8
        }
    }
    // arbitrary mapping from a state parameter in the main loop. Tune this to
    // make things "fun". Want to make about 8 toggles equal one level, each toggle
    // increments by 4.
    pub fn from_param(param: u8) -> MutationRate {
        let rate = match param {
            0..30 => Self::Baseline,
            30..70 => Self::Elevated,
            70..120 => Self::Radioactive,
            _ => Self::Apocalyptic,
        };
        rate
    }
}

#[derive(
    Default, Pod, Zeroable, Copy, Clone, Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize,
)]
#[repr(C)]
pub struct Haploid {
    pub cd_period: u8,
    pub cd_rate: u8,
    pub cd_dir: u8,
    pub sat: u8,
    pub hue_ratedir: u8,
    pub hue_base: u8,
    pub hue_bound: u8,
    pub chaser: u8,
    pub nonlin: u8,
}
impl Haploid {
    pub fn from_rand() -> Self {
        let mut h = Haploid::default();
        h.cd_period = rand::thread_rng().gen_range(0..=6);
        h.cd_rate = rand::thread_rng().gen();
        h.cd_dir = rand::thread_rng().gen();
        h.sat = rand::thread_rng().gen();
        h.hue_ratedir = rand::thread_rng().gen();
        h.hue_base = rand::thread_rng().gen();
        h.hue_bound = rand::thread_rng().gen();
        h.chaser = rand::thread_rng().gen();
        h.nonlin = rand::thread_rng().gen();
        h
    }

    pub fn from_type(badge_type: &BadgeType) -> Self {
        let mut h = Haploid::default();
        h.cd_period = rand::thread_rng().gen_range(0..=badge_type.cd_period_max());
        h.cd_rate = rand::thread_rng().gen();
        h.cd_dir = rand::thread_rng().gen_range(badge_type.cd_dir_range());
        h.sat = rand::thread_rng().gen_range(badge_type.sat_range());
        h.hue_ratedir = rand::thread_rng().gen();
        h.hue_base = rand::thread_rng().gen_range(badge_type.hue_range());
        if *badge_type == BadgeType::Goon {
            // ensure that red is always part of the Goon pallette
            h.hue_base = 0;
        }
        h.hue_bound = rand::thread_rng().gen_range(h.hue_base..=*badge_type.hue_range().end());
        if *badge_type == BadgeType::Uber {
            h.hue_bound = 255;
        }
        h.chaser = rand::thread_rng().gen_range(badge_type.chaser_range());
        h.nonlin = rand::thread_rng().gen_range(badge_type.nonlin_range());
        h
    }

    pub fn serialize(&self) -> Vec<u8> {
        bytemuck::bytes_of(self).to_vec()
    }

    pub fn deserialize(bytes: &[u8]) -> Option<Self> {
        bytemuck::try_from_bytes(bytes).ok().copied()
    }

    /// always returns a length-4 serialization, suitable for Xous args
    pub fn serialize_u32(&self) -> [u32; 4] {
        let bytes = bytemuck::bytes_of(self);
        let mut padded = [0u8; 16];
        padded[..bytes.len()].copy_from_slice(bytes);
        let mut out = [0u32; 4];
        for (i, chunk) in padded.chunks(4).enumerate() {
            out[i] = u32::from_le_bytes(chunk.try_into().unwrap());
        }
        out
    }

    /// gracefully handles 4 args by truncating the extra args that are just padding inserted by
    /// serialize_u32()
    pub fn deserialize_u32(words: &[u32]) -> Option<Self> {
        let bytes: Vec<u8> = words
            .iter()
            .flat_map(|w| w.to_le_bytes())
            .take(std::mem::size_of::<Haploid>())
            .collect();
        bytemuck::try_from_bytes(&bytes).ok().copied()
    }
}

#[derive(Debug, Copy, Clone, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct Diploid(pub [Haploid; 2]);

impl Diploid {
    // computes the phenotypic expression of the diploid genome by blending the haploid pairs according
    // to rules that create dominant/recessive traits. In reality you don't get another strand of DNA, you get
    // proteins, but 'meh'. Close enough for computer science.
    pub fn phenotype(&self) -> Haploid {
        let mut e = Haploid {
            // add -> periodicity tends toward the mean, capped at 6 periods
            cd_period: ((self.0[0].cd_period + self.0[1].cd_period) / 2).min(6),
            // average -> rate tends toward the mean
            cd_rate: ((self.0[0].cd_rate as u16 + self.0[1].cd_rate as u16) / 2) as u8,
            // add -> clockwise direction is dominant
            cd_dir: self.0[0].cd_dir.saturating_add(self.0[1].cd_dir),
            // add -> saturated colors are dominant
            sat: self.0[0].sat.saturating_add(self.0[1].sat),
            // inverse add -> slower hue cycling is dominant
            hue_ratedir: (2
                + (14
                    - self.0[0]
                        .hue_ratedir
                        .saturating_add(self.0[1].hue_ratedir)
                        .min(14)))
                % 14,
            // min -> wider color range is dominant
            hue_base: self.0[0].hue_base.min(self.0[1].hue_base),
            // max -> wider color range is dominant
            hue_bound: self.0[0].hue_bound.max(self.0[1].hue_bound),
            // chaser -> large chaser values (which is no chaser) is dominant
            chaser: self.0[0].chaser.saturating_add(self.0[1].chaser),
            // nonlin -> brightness correction is dominant
            nonlin: self.0[0].chaser.saturating_add(self.0[1].nonlin),
        };
        // ensure that the bound is always bigger than the base
        e.hue_bound = e.hue_bound.max(e.hue_base);
        e
    }
    pub fn send(&self, conn: xous::CID, opcode: usize) {
        let buf = xous_ipc::Buffer::into_buf(self.clone()).unwrap();
        buf.lend(conn, opcode as u32).unwrap();
    }
    pub fn receive(mem: &xous::MemoryMessage) -> Self {
        let buffer = unsafe { xous_ipc::Buffer::from_memory_message(mem) };
        let gene = buffer.to_original::<Diploid, _>().unwrap();
        gene
    }
    pub fn meiosis(&self) -> Haploid {
        let mut gamete = Haploid::default();
        let parent: usize = rand::thread_rng().gen_range(0..2);
        gamete.cd_period = self.0[parent].cd_period;
        gamete.cd_rate = self.0[parent].cd_rate;
        gamete.cd_dir = self.0[parent].cd_dir;

        gamete.sat = self.0[rand::thread_rng().gen_range(0..2)].sat;

        let parent: usize = rand::thread_rng().gen_range(0..2);
        gamete.hue_ratedir = self.0[parent].hue_ratedir;
        gamete.hue_base = self.0[parent].hue_base;
        gamete.hue_bound = self.0[parent].hue_bound;

        gamete.chaser = self.0[rand::thread_rng().gen_range(0..2)].chaser;
        gamete.nonlin = self.0[rand::thread_rng().gen_range(0..2)].nonlin;
        gamete
    }
}

impl Diploid {
    pub fn serialize(&self) -> Vec<u8> {
        [self.0[0].serialize(), self.0[1].serialize()]
            .concat()
            .to_vec()
    }
    pub fn deserialize(bytes: &[u8]) -> Option<Self> {
        let size = std::mem::size_of::<Haploid>();
        if bytes.len() < size * 2 {
            return None;
        }
        let a = Haploid::deserialize(&bytes[..size])?;
        let b = Haploid::deserialize(&bytes[size..size * 2])?;
        Some(Diploid([a, b]))
    }
}

// Haploid and Diploid *must* be thread-safe. Don't add stuff to it that's not Send + Sync
#[allow(dead_code)]
trait AssertSendSync: Send + Sync {}
impl AssertSendSync for Haploid {}
impl AssertSendSync for Diploid {}

pub fn init_light_gene(badge_type: BadgeType) {
    let pddb = pddb::Pddb::new();
    let mut badge_key = pddb
        .get(
            DC34_DICT,
            DC34_BADGE,
            None,
            true,
            true,
            Some(1),
            None::<fn()>,
        )
        .expect("couldn't get PDDB key");
    badge_key.write(&[badge_type as u8]).ok();

    let individual = Diploid([
        Haploid::from_type(&badge_type),
        Haploid::from_type(&badge_type),
    ]);
    log::info!("Created individual: {:?} / {:?}", badge_type, individual);
    save_light_gene(individual);
}

/// A potentially time-expensive operation to read the light gene from PDDB storage
pub fn get_light_gene() -> Option<Diploid> {
    let pddb = pddb::Pddb::new();
    let mut gene_key = pddb
        .get(
            DC34_DICT,
            DC34_GENE,
            None,
            true,
            true,
            Some(1),
            None::<fn()>,
        )
        .expect("couldn't get PDDB key");
    let mut buf = [0u8; std::mem::size_of::<Haploid>() * 2];
    gene_key.read_exact(&mut buf).ok()?;
    let ret = Diploid::deserialize(&buf);
    log::debug!("Read in gene: {:?}", ret);
    ret
}

/// A potentially time-expensive operation to commit the light gene to PDDB storage
pub fn save_light_gene(gene: Diploid) {
    let pddb = pddb::Pddb::new();
    let mut gene_key = pddb
        .get(
            DC34_DICT,
            DC34_GENE,
            None,
            true,
            true,
            Some(1),
            None::<fn()>,
        )
        .expect("couldn't get PDDB key");
    gene_key.write_all(&gene.serialize()).ok();
}

pub fn save_k0(k0: &[u8; 32]) {
    let pddb = pddb::Pddb::new();
    let mut k0_key = pddb
        .get(
            DC34_DICT,
            DC34_SECRET,
            None,
            true,
            true,
            Some(32),
            None::<fn()>,
        )
        .expect("couldn't get PDDB key");
    k0_key.write_all(k0).ok();

    // this also resets the BadgeType when the key is assigned
    // this is safe because the key is only known to the assigner -
    // if someone calls this without the correct key, they lose
    // access to the light exchange protocol, and also lose the key.
    // In which case, sure, you can show colors on your badge that
    // aren't supposed to be yours, but you can't share them with
    // anyone else.
    let mut badge_key = pddb
        .get(
            DC34_DICT,
            DC34_BADGE,
            None,
            true,
            true,
            Some(1),
            None::<fn()>,
        )
        .expect("couldn't get PDDB key");
    badge_key.write(&[BadgeType::None as u8]).ok();
    let mut key = pddb
        .get(
            DC34_DICT,
            DC34_TOUR,
            None,
            true,
            true,
            Some(1),
            None::<fn()>,
        )
        .expect("couldn't get PDDB key");
    key.write(&[0]).ok();
    pddb.sync().ok();
}

pub fn get_k0() -> Option<[u8; 32]> {
    let mut k0_buf = [0u8; 32];
    let pddb = pddb::Pddb::new();
    let mut k0_key = pddb
        .get(
            DC34_DICT,
            DC34_SECRET,
            None,
            true,
            true,
            Some(1),
            None::<fn()>,
        )
        .expect("couldn't get PDDB key");
    let result = k0_key.read(&mut k0_buf);
    match result {
        Ok(len) => {
            if len == 32 {
                Some(k0_buf)
            } else {
                None
            }
        }
        Err(_e) => None,
    }
}

pub fn mutate(gamete: &mut Haploid, rate: MutationRate) {
    let bits = rate.to_bit_changes();

    if rate.roll() {
        gamete.cd_period = mutation_func(gamete.cd_period, bits) % 7;
    }
    if rate.roll() {
        gamete.cd_rate = mutation_func(gamete.cd_rate, bits);
    }
    if rate.roll() {
        gamete.cd_dir = mutation_func(gamete.cd_dir, bits);
    }
    if rate.roll() {
        gamete.sat = mutation_func(gamete.sat, bits);
    }
    if rate.roll() {
        gamete.hue_ratedir = mutation_func(gamete.hue_ratedir, bits);
    }
    if rate.roll() {
        gamete.hue_base = mutation_func(gamete.hue_base, bits);
    }
    if rate.roll() {
        gamete.hue_bound = mutation_func(gamete.hue_bound, bits);
    }
    if rate.roll() {
        gamete.chaser = mutation_func(gamete.chaser, bits);
    }
    if rate.roll() {
        gamete.nonlin = mutation_func(gamete.nonlin, bits);
    }
}

fn mutation_func(gene: u8, bits: u8) -> u8 {
    gray_decode(gray_encode(gene) ^ (bits << (rand::thread_rng().gen_range(0..=7))))
}

fn gray_encode(n: u8) -> u8 {
    n ^ (n >> 1)
}

fn gray_decode(mut n: u8) -> u8 {
    let mut p = n;
    while n >> 1 != 0 {
        n >>= 1;
        p ^= n;
    }
    p
}
