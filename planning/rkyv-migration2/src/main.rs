mod buffer;
use buffer::*;

mod testcases;
use rkyv::{deserialize, rancor::Error, with::Identity, Archive, Deserialize, Serialize};
use testcases::*;

// A test structure with rkyv derives
#[derive(Archive, Deserialize, Serialize, Debug, PartialEq)]
#[rkyv(
    // This will generate a PartialEq impl between our unarchived
    // and archived types
    compare(PartialEq),
    // Derives can be passed through to the generated type:
    derive(Debug),
)]
struct Test {
    int: u8,
    string: String,
    option: Option<Vec<i32>>,
    string2: String,
}

fn main() {
    println!("Size of Test: {}", core::mem::size_of::<Test>());

    // with an alloc
    let value = Test {
        int: 42,
        string: "The quick brown fox jumps over the lazy dogs".to_string(),
        option: Some(vec![1, 2, 3, 4]),
        string2: "Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor incididunt ut labore et dolore magna aliqua. Ut enim ad minim veniam, quis nostrud exercitation ullamco laboris nisi ut aliquip ex ea commodo consequat. Duis aute irure dolor in reprehenderit in voluptate velit esse cillum dolore eu fugiat nulla pariatur. Excepteur sint occaecat cupidatat non proident, sunt in culpa qui officia deserunt mollit anim id est laborum.".to_string(),
    };

    let buf = Buffer::into_buf::<Identity, Test>(&value); // AsBox

    let f = buf.as_flat::<Test, _>().unwrap();
    println!("f: {:?}", f);

    let t1 = buf.to_original::<Test, _, Error>().unwrap();
    println!("t copy 1: {:?}", t1);

    println!("buf.slice: {:x?}", &buf[..buf.used()]);
    let archived = unsafe { rkyv::access_unchecked::<ArchivedTest>(&buf) };
    println!("archived: {:?}", archived);
    let t = rkyv::deserialize::<Test, Error>(archived).unwrap();
    println!("t: {:?}", t);
    assert_eq!(t.string, value.string);

    use rkyv::api::high::to_bytes_with_alloc;
    use rkyv::ser::allocator::Arena;
    let mut arena = Arena::new();

    // let de = buf.to_original::<Test, rkyv::rancor::Error>().unwrap();
    let bytes = to_bytes_with_alloc::<_, Error>(&value, arena.acquire()).unwrap();
    let archived = unsafe { rkyv::access_unchecked::<ArchivedTest>(&bytes[..]) };
    assert_eq!(archived, &value);
    let deserialized = deserialize::<Test, Error>(archived).unwrap();
    assert_eq!(deserialized.string2, value.string2);

    let mut tv = testcases::TextView::new(
        Gid { gid: [6, 7, 8, 9] },
        TextBounds::BoundingBox(Rectangle {
            tl: Point { x: 0, y: 0 },
            br: Point { x: 100, y: 100 },
            style: DrawStyle { fill_color: None, stroke_color: None, stroke_width: 1 },
        }),
    );
    // make a lorem ipsum text
    tv.text.push_str(&value.string2);
    println!("Tv bounds computed before: {:?}", tv.bounds_computed);

    let mut tv_buf = Buffer::into_buf::<Identity, _>(&tv);
    match tv_buf.lend_mut(1, 1) {
        Ok(r) => {
            println!("Returned {:?}", r);
            let mut tv_orig = tv_buf.to_original::<TextView, _, Error>().unwrap();
            println!("bounds_computed: {:?}", tv_orig.bounds_computed);
            assert_eq!(tv_orig.bounds_computed.unwrap().tl.x, 42);
            assert_eq!(tv_orig.bounds_computed.unwrap().tl.y, 42);
            println!("now embedded in routine");
            draw_textview(1, &mut tv_orig).ok();
        }
        Err(e) => {
            println!("Test failed with error {:?}", e);
            panic!("test failed");
        }
    }

}
