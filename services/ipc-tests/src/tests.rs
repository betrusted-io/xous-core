//! xous-ipc Buffer test battery.
//!
//! Every test that depends on exact archive sizes checks `into_buf().used()`
//! against an in-binary probe (the same IoWriter<io::Sink> counting writer
//! `into_buf`'s growth path uses) rather than hardcoded byte counts: riscv32
//! and x86 have different `usize`/rel-ptr widths, and pinning host-derived
//! sizes here would be wrong on the very target we are testing. The probe
//! equality itself is what the growth patch promises, so asserting
//! used == probe on this target closes the loop the host test could not.
//!
//! IMPORTANT (mirrors net-tests authoring rules): never nest heap-backed
//! types inside `Vec` elements (`Vec<String>`, `Vec<Vec<u8>>`, ...). The fixed
//! 256-byte `SubAllocator` scratch in `into_buf`/`replace` holds one 4-byte
//! resolver per pending out-of-line element, capping such nests at 64
//! elements. Flat `Vec<POD>`, `String`, and POD inline fields are safe.

use rkyv::rancor::Failure;
use rkyv::ser::writer::IoWriter;
use rkyv::ser::{Positional, allocator::SubAllocator};
use rkyv::{Archive, Deserialize, Serialize};
use std::io;

use ipc_echo::{EchoPayload, OP_ECHO, SERVER_NAME};

const PAGE: usize = 4096;

// ---------- test types (authoring rules above) ----------

#[derive(Archive, Serialize, Deserialize, Debug, PartialEq, Clone)]
pub enum Kind {
    None,
    Counts(Vec<u32>),
    Text(String),
}

#[derive(Archive, Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct Composite {
    pub head: u32,
    pub data: Vec<u8>,
    pub label: String,
    pub blob: [[u8; 32]; 8],
    pub opt: Option<Vec<u8>>,
    pub kind: Kind,
}

#[derive(Archive, Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct Pod {
    pub a: u32,
    pub b: i64,
    pub c: [u8; 32],
    pub d: [[u8; 16]; 4],
}

#[derive(Archive, Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct Empty;

// ---------- helpers ----------

/// Count a value's serialized length with no backing store: exactly what
/// `into_buf`'s probe does internally. The bound mirrors `into_buf`'s.
pub fn probe_len<T>(v: &T) -> usize
where
    T: for<'a> Serialize<
            rkyv::rancor::Strategy<
                rkyv::ser::Serializer<IoWriter<io::Sink>, SubAllocator<'a>, ()>,
                Failure,
            >,
        >,
{
    let mut scratch = [core::mem::MaybeUninit::<u8>::uninit(); 256];
    let writer = IoWriter::new(io::sink());
    let alloc = SubAllocator::new(&mut scratch);
    rkyv::api::low::to_bytes_in_with_alloc::<_, _, Failure>(v, writer, alloc)
        .map(|w| w.pos())
        .expect("probe: value did not serialize")
}

fn page_round(n: usize) -> usize {
    (n + (PAGE - 1)) & !(PAGE - 1)
}

/// Assert the growth-path invariants on a buffer made by `into_buf`:
/// `used` equals the probe length exactly, and `capacity` is the page-rounded
/// allocation that just fits it.
fn check_sized<T>(buf: &xous_ipc::Buffer, expected: &T)
where
    T: Archive + PartialEq + core::fmt::Debug + Probeable,
    <T as Archive>::Archived: rkyv::Portable
        + for<'a> rkyv::bytecheck::CheckBytes<rkyv::api::low::LowValidator<'a, rkyv::rancor::Error>>
        + Deserialize<T, rkyv::rancor::Strategy<rkyv::de::Pool, rkyv::rancor::Error>>,
{
    let probe = expected.probe();
    assert_eq!(buf.used(), probe, "used must equal probe length exactly");
    assert_eq!(
        buf.capacity(),
        page_round(probe.max(PAGE)),
        "capacity must be the page-rounded allocation"
    );
    let back: T = buf
        .to_original()
        .expect("roundtrip: valid buffer must deserialize");
    assert_eq!(&back, expected, "roundtrip must be lossless");
}

/// Blanket so `check_sized` can take any serializable payload and probe it.
pub trait Probeable {
    fn probe(&self) -> usize;
}
impl<T> Probeable for T
where
    T: for<'a> Serialize<
            rkyv::rancor::Strategy<
                rkyv::ser::Serializer<IoWriter<io::Sink>, SubAllocator<'a>, ()>,
                Failure,
            >,
        >,
{
    fn probe(&self) -> usize {
        probe_len(self)
    }
}

fn composite(data_len: usize, label_len: usize, kind: Kind) -> Composite {
    let mut st = 0x5EED_F00D ^ (data_len as u32);
    let mut byte = move || {
        st ^= st << 13;
        st ^= st >> 17;
        st ^= st << 5;
        st as u8
    };
    Composite {
        head: data_len as u32 ^ 0xBEEF,
        data: (0..data_len).map(|_| byte()).collect(),
        label: (0..label_len).map(|_| (b'a' + byte() % 26) as char).collect(),
        blob: [[byte(); 32]; 8],
        opt: if data_len % 2 == 0 { Some((0..16).map(|_| byte()).collect()) } else { None },
        kind,
    }
}

// ---------- tests ----------

/// The core promise of the growth patch: `into_buf().used()` is EXACTLY the
/// probe length on riscv32, for POD (fast path) and heap-backed payloads.
pub fn t_probe_exact_pod() {
    let pod = Pod {
        a: 0xDEAD_BEEF,
        b: -0x1234_5678_9ABC_DEF0,
        c: [0x5A; 32],
        d: [[0xA5; 16]; 4],
    };
    let buf = xous_ipc::Buffer::into_buf(pod.clone()).expect("pod: into_buf");
    // POD fits one page: fast path must have been taken (no second attempt)
    assert_eq!(buf.capacity(), PAGE, "POD must land in the single-page fast path");
    check_sized(&buf, &pod);
    // as_flat sees the same bytes zero-copy
    let flat = buf.as_flat::<Pod, _>().expect("pod: as_flat");
    assert_eq!(flat.a.to_native(), 0xDEAD_BEEF);
}

/// Page-boundary battery for the fast-path/slow-path seam: sizes whose
/// serialized form straddles a page must all produce exact `used` and correct
/// page-rounded `capacity`.
pub fn t_page_boundaries() {
    for data_len in [0usize, 1, 4064, 4092, 4093, 4096, 4099, 4100, 8192, 8208, 12304] {
        let v: Vec<u8> = composite(data_len, 0, Kind::None).data;
        let buf = xous_ipc::Buffer::into_buf(v.clone())
            .unwrap_or_else(|e| panic!("vec {} into_buf: {:?}", data_len, e));
        check_sized(&buf, &v);
    }
}

/// String payloads over one page exercise the probe with a different type
/// shape (UTF-8 validation on the way back).
pub fn t_string_multipage() {
    let s: String = (0..10_000).map(|i| (b' ' + (i % 95) as u8) as char).collect();
    let buf = xous_ipc::Buffer::into_buf(s.clone()).expect("string into_buf");
    check_sized(&buf, &s);
}

/// The multi-page composite from the host verification (sizes re-derived for
/// riscv32): inline arrays + Vec + String + Option<Vec> + enum, >4 pages.
pub fn t_composite_multipage() {
    let c = composite(12_000, 3_000, Kind::Counts((0..2000).collect()));
    let buf = xous_ipc::Buffer::into_buf(c.clone()).expect("composite into_buf");
    assert!(buf.capacity() > 4 * PAGE, "test payload must be multi-page");
    check_sized(&buf, &c);
    // zero-copy view must agree with the owning roundtrip
    let flat = buf.as_flat::<Composite, _>().expect("composite as_flat");
    assert_eq!(flat.data.len(), 12_000);
    assert_eq!(flat.label.len(), 3_000);
}

/// The growth path's original motivating failure: a `Vec` whose `size_of` is
/// 12 bytes but whose payload crosses several pages. Also pins `used` after a
/// fast-path overflow (retry must not report the first attempt's position).
pub fn t_vec_over_page() {
    for n in [4_097usize, 20_000, 100_000] {
        let v: Vec<u8> = (0..n).map(|i| (i % 251) as u8).collect();
        let buf = xous_ipc::Buffer::into_buf(v.clone()).expect("vec over page into_buf");
        check_sized(&buf, &v);
    }
}

/// The ceiling is gone: a full megabyte payload must allocate and round-trip.
/// Kernel pages materialize lazily, so this costs faults + zeroing but no
/// artificial cap.
pub fn t_ceiling_removed_1mib() {
    let v: Vec<u8> = (0..1024 * 1024).map(|i| (i >> 3) as u8).collect();
    let buf = xous_ipc::Buffer::into_buf(v.clone()).expect("1 MiB payload must serialize");
    check_sized(&buf, &v);
    assert!(buf.capacity() >= 1024 * 1024);
}

/// Zero-sized root: must terminate cleanly (the no-grow guard / exact-size
/// path), never spin. We accept either an empty buffer or a clean error --
/// what we must NOT get is a hang or a panic.
pub fn t_zero_size() {
    let r = xous_ipc::Buffer::into_buf(Empty);
    match r {
        Ok(buf) => assert!(buf.used() < PAGE, "empty payload cannot fill a page"),
        Err(e) => log::info!("zero-size into_buf returned clean error: {:?}", e),
    }
}

/// Checked-access errors from the rkyv `access` patch: wrong type and
/// truncated buffer must both surface SerializationError, never UB/panic.
pub fn t_checked_access_errors() {
    let c = composite(100, 10, Kind::Text("hello".into()));
    let mut buf = xous_ipc::Buffer::into_buf(c).expect("access: into_buf");

    // wrong type: different layout, must be rejected
    let r = buf.as_flat::<Composite2, _>();
    assert_eq!(r.err(), Some(xous::Error::SerializationError), "wrong-type as_flat");
    let r = buf.to_original::<Composite2, _>();
    assert_eq!(r.err(), Some(xous::Error::SerializationError), "wrong-type to_original");

    // truncated window: a second handle over fewer bytes must not validate
    let (pages, len, used) = unsafe { buf.to_raw_parts() };
    unsafe {
        let trunc = xous_ipc::Buffer::from_raw_parts(pages, len, used - 4);
        assert_eq!(
            trunc.as_flat::<Composite, _>().err(),
            Some(xous::Error::SerializationError),
            "truncated as_flat"
        );
    }
    // the intact buffer still works
    let back: Composite = buf.to_original().expect("access: intact roundtrip");
    assert_eq!(back.head, 100 ^ 0xBEEF);
}

#[derive(Archive, Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct Composite2 {
    pub totally: String,
    pub different: u128,
    pub shape: Vec<u64>,
}

/// `replace()` overflow from the error-type patch: returns Err instead of
/// panicking, leaves `used` at its previous value, and the buffer remains
/// usable afterwards.
pub fn t_replace_overflow() {
    let mut buf = xous_ipc::Buffer::new(PAGE);
    let small = vec![1u8; 64];
    buf.replace(small.clone()).expect("small replace");
    assert_eq!(buf.used(), probe_len(&small));

    let big = vec![2u8; 8_000]; // does not fit in one page
    assert_eq!(
        buf.replace(big.clone()).err(),
        Some(xous::Error::SerializationError),
        "overflowing replace must error, not panic"
    );
    assert_eq!(buf.used(), probe_len(&small), "failed replace must leave used unchanged");

    // buffer still usable
    buf.replace(small.clone()).expect("replace after overflow");
    let back: Vec<u8> = buf.to_original().expect("roundtrip after overflow");
    assert_eq!(back, small);
    let _ = big;
}

/// `volatile_clear()` clears the buffer (used region reads back zero) and the
/// buffer stays usable. This is the belt-and-suspenders full-slice clear.
pub fn t_volatile_clear() {
    let mut buf = xous_ipc::Buffer::into_buf(vec![0xA5u8; 2000]).expect("clear: into_buf");
    assert!(buf.iter().any(|&b| b != 0), "payload region should contain nonzero bytes");
    buf.volatile_clear();
    assert!(buf.iter().all(|&b| b == 0), "volatile_clear must zero the used region");
    buf.replace(vec![0x5Au8; 100]).expect("clear: reuse");
    assert!(buf.iter().any(|&b| b == 0x5A), "buffer must be reusable after clear");
}

/// Echo round-trips through a real kernel MutableBorrow: small (single page,
/// fast path) and large (multi-page, probe-sized buffer, response with a
/// different length forcing an updated `used` on return).
pub fn t_echo_roundtrip() {
    for data_len in [64usize, 300_000] {
        let xns = xous_names::XousNames::new().expect("echo: xns");
        let cid = xns
            .request_connection(SERVER_NAME)
            .unwrap_or_else(|e| panic!("echo {} connect: {:?}", data_len, e));
        let req = EchoPayload {
            tag: 0xC0FFEE + data_len as u32,
            data: (0..data_len).map(|i| (i % 253) as u8).collect(),
            label: format!("echo request len {}", data_len),
        };
        let mut buf = xous_ipc::Buffer::into_buf(req).expect("echo: into_buf");
        buf.lend_mut(cid, OP_ECHO as u32).expect("echo: lend_mut");
        // The response is shorter than the request (server drops the label):
        // a correct `used` here proves the kernel->client returned-offset path.
        let resp: EchoPayload = buf.to_original().expect("echo: response deserializes");
        assert_eq!(resp.tag, 0xC0FFEE + data_len as u32, "echo: tag");
        assert_eq!(resp.data.len(), data_len, "echo: data length");
        let mut want: Vec<u8> = (0..data_len).map(|i| (i % 253) as u8).collect();
        want.reverse();
        assert_eq!(resp.data, want, "echo: payload must arrive reversed");
        assert!(resp.label.is_empty(), "echo: server drops the label (length changed)");
    }
}

pub type TestFn = fn();

/// (name, test) registry. Names are stable sentinels; never reuse one.
pub fn all_tests() -> Vec<(&'static str, TestFn)> {
    vec![
        ("probe_exact_pod", t_probe_exact_pod as TestFn),
        ("page_boundaries", t_page_boundaries as TestFn),
        ("string_multipage", t_string_multipage as TestFn),
        ("composite_multipage", t_composite_multipage as TestFn),
        ("vec_over_page", t_vec_over_page as TestFn),
        ("ceiling_removed_1mib", t_ceiling_removed_1mib as TestFn),
        ("zero_size", t_zero_size as TestFn),
        ("checked_access_errors", t_checked_access_errors as TestFn),
        ("replace_overflow", t_replace_overflow as TestFn),
        ("volatile_clear", t_volatile_clear as TestFn),
        ("echo_roundtrip", t_echo_roundtrip as TestFn),
    ]
}
