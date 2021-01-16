use core::cmp::Ordering;
use heapless::binary_heap::{BinaryHeap, Max};
use heapless::FnvIndexMap;
use heapless::Vec;
use heapless::consts::*;
use graphics_server::{Rectangle, Point};

#[derive(Debug, Copy, Clone)]
pub enum CanvasState {
    // the initial state of every Canvas. Not drawable.
    Created,
    // this state indicates the Canvas can be drawn, but has yet to be.
    DrawableDirty,
    // this state indicates the Canvas has been drawn.
    DrawableDrawn,
    // indicates that the Canvas is not drawable, but needs to be defaced
    NotDrawableDirty,
    // indicates that the Canvas is not drawable, and has been defaced
    NotDrawableDefaced,
}

/// A rectangular region that defines a top-left zero relative offset for graphical items
/// and a bottom-right point that defines a clipping area for things drawn inside.
#[derive(Debug, Copy, Clone)]
#[repr(C)]
pub struct Canvas {
    // screen coordinates of the clipping region
    clip_rect: Rectangle,

    // trust level, 255 is most trusted
    trust_level: u8,

    state: CanvasState,

    // unique, random identifier for the Canvas
    gid: [u32; 4],

    // enables scroll/pan of objects within a region
    pan_offset: Point,
}

// we need the "screen" parameter so we can turn off drawing to canvases that are off-screen
pub fn recompute_canvases(mut canvases: BinaryHeap<Canvas, U32, Max>, screen: Rectangle) -> (BinaryHeap<Canvas, U32, Max>, FnvIndexMap<[u32; 4], Canvas, U32>) {
    let mut higher_clipregions: BinaryHeap<Canvas, U32, Max> = BinaryHeap::new();

    let mut trust_level: u8 = 255;
    // canvases is a Max heap keyed on trust, so popping the elements off will return them sorted from most to least trusted
    if let Some(c) = canvases.pop() {
        let mut canvas = c.clone();

        let mut drawable: bool = true;
        let clip_region = canvas.clip_rect();
        if trust_level < canvas.trust_level() {
            trust_level = canvas.trust_level();
        }
        // note that this .iter() is *not* sorted by trust level, but all elements will be of greater than or equal to the current trust level
        for &region in higher_clipregions.iter() {
            // regions of the same trust level can draw over each other. Draw order is arbitrary.
            if region.clip_rect().intersects(clip_region) && (region.trust_level() < trust_level) ||
               !region.clip_rect().intersects(screen) {
                drawable = false;
            }
        }
        match canvas.state() {
            CanvasState::Created =>
               if drawable { canvas.set_state(CanvasState::DrawableDirty) }
               else { canvas.set_state(CanvasState::NotDrawableDirty) },
            CanvasState::DrawableDirty | CanvasState::DrawableDrawn =>
               if !drawable { canvas.set_state(CanvasState::NotDrawableDirty) },
            CanvasState::NotDrawableDefaced | CanvasState::NotDrawableDirty =>
               if drawable { canvas.set_state(CanvasState::DrawableDirty) }
        }
        higher_clipregions.push(canvas).unwrap();
    }

    let mut map: FnvIndexMap<[u32; 4], Canvas, U32> = FnvIndexMap::new();
    for &c in higher_clipregions.iter() {
        map.insert(c.gid(), c).unwrap();
    }

    (higher_clipregions, map)
}

impl Canvas {
    pub fn new(clip_rect: Rectangle, trust_level: u8,
        trng_conn: xous::CID, pan_offset: Option<Point>) -> Result<Canvas, xous::Error> {

        let mut gid: [u32; 4] = [0; 4];
        let g: u64 = trng::get_u64(trng_conn)?;
        gid[0] = g as u32;
        gid[1] = (g >> 32) as u32;
        let g: u64 = trng::get_u64(trng_conn)?;
        gid[2] = g as u32;
        gid[3] = (g >> 32) as u32;

        Ok(if pan_offset.is_some() {
            Canvas {
                clip_rect, trust_level, state: CanvasState::Created, gid, pan_offset: pan_offset.unwrap()
            }
        } else {
            Canvas {
                clip_rect, trust_level, state: CanvasState::Created, gid, pan_offset: Point::new(0, 0)
            }
        })
    }
    pub fn clip_rect(&self) -> Rectangle { self.clip_rect }
    pub fn gid(&self) -> [u32; 4] { self.gid }
    pub fn trust_level(&self) -> u8 { self.trust_level }
    pub fn state(&self) -> CanvasState { self.state }
    pub fn set_state(&mut self, state: CanvasState) { self.state = state; }
}

impl Ord for Canvas {
    fn cmp(&self, other: &Self) -> Ordering {
        self.trust_level.cmp(&other.trust_level)
    }
}
impl PartialOrd for Canvas {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl PartialEq for Canvas {
    fn eq(&self, other: &Self) -> bool {
        self.trust_level == other.trust_level
    }
}
impl Eq for Canvas {}
