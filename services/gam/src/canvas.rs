use core::cmp::Ordering;
use heapless::binary_heap::{BinaryHeap, Max};
use heapless::FnvIndexMap;
use heapless::Vec;
use heapless::consts::*;
use graphics_server::{Rectangle, Point};
use xous::ipc::Sendable;
use log::{error, info};

use crate::api::*;

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum CanvasState {
    // the initial state of every Canvas. Not drawable.
    Created,
    // this state indicates the Canvas can be drawn, and may or may not need to be flushed to the screen.
    DrawableDirty,
    // this state indicates the Canvas has been flushed to the screen.
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
    gid: Gid,

    // enables scroll/pan of objects within a region
    pan_offset: Point,
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
                clip_rect, trust_level, state: CanvasState::Created, gid: Gid::new(gid), pan_offset: pan_offset.unwrap()
            }
        } else {
            Canvas {
                clip_rect, trust_level, state: CanvasState::Created, gid: Gid::new(gid), pan_offset: Point::new(0, 0)
            }
        })
    }
    pub fn clip_rect(&self) -> Rectangle { self.clip_rect }
    pub fn gid(&self) -> Gid { self.gid }
    pub fn trust_level(&self) -> u8 { self.trust_level }
    pub fn state(&self) -> CanvasState { self.state }
    pub fn is_drawable(&self) -> bool {
        if self.state == CanvasState::DrawableDirty || self.state == CanvasState::DrawableDrawn {
            true
        } else {
            false
        }
    }
    pub fn set_drawable(&mut self, drawable: bool) {
        if drawable {
            if self.state != CanvasState::DrawableDrawn {
                self.state = CanvasState::DrawableDirty;
            }
        } else {
            if self.state != CanvasState::NotDrawableDefaced {
                self.state = CanvasState::NotDrawableDirty;
            }
        }
    }
    // call this after the screen has been flushed
    pub fn do_flushed(&mut self) -> Result<(), xous::Error> {
        if self.state == CanvasState::DrawableDirty || self.state == CanvasState::DrawableDrawn {
            self.state = CanvasState::DrawableDrawn;
            Ok(())
        } else if self.state == CanvasState::NotDrawableDefaced {
            Ok(())
        } else {
            error!("Canvas: flush happened before not drawable regions were defaced, or before initialized!");
            Err(xous::Error::UseBeforeInit)
        }
    }
    pub fn do_drawn(&mut self) -> Result<(), xous::Error> {
        if self.state == CanvasState::DrawableDirty || self.state == CanvasState::DrawableDrawn {
            self.state = CanvasState::DrawableDirty;
            Ok(())
        } else {
            error!("Canvas: attempt to draw on regions that are not drawable, or not initialized!");
            Err(xous::Error::AccessDenied)
        }
    }
    pub fn do_defaced(&mut self) -> Result<(), xous::Error> {
        if self.state == CanvasState::NotDrawableDirty {
            self.state = CanvasState::NotDrawableDefaced;
            Ok(())
        } else if self.state == CanvasState::DrawableDirty || self.state == CanvasState::DrawableDrawn {
            info!("Canvas: drawable region was defaced. Allowing it, but this could be a logic bug");
            Ok(())
        } else {
            error!("Canvas: attempt to deface region already defaced, or not initialized!");
            Err(xous::Error::DoubleFree)
        }
    }
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



// we use the "screen" parameter to determine when we can turn off drawing to canvases that are off-screen
pub fn recompute_canvases(mut canvases: FnvIndexMap<Gid, Canvas, U32>, screen: Rectangle) -> FnvIndexMap<Gid, Canvas, U32> {
    // first, sort canvases by trust_level. Canvas implements ord/eq based on the trust_level attribute
    // so jush pushing it into a max binary heap does the trick.
    let mut sorted_clipregions: BinaryHeap<Canvas, U32, Max> = BinaryHeap::new();
    for (_, &c) in canvases.iter() {
        sorted_clipregions.push(c);
    }

    // now, descend through trust levels and compute intersections, putting the updated drawable states into higher_clipregions
    let mut higher_clipregions: BinaryHeap<Canvas, U32, Max> = BinaryHeap::new();
    let mut trust_level: u8 = 255;
    // sorted_clipregions is a Max heap keyed on trust, so popping the elements off will return them sorted from most to least trusted
    if let Some(c) = sorted_clipregions.pop() {
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
        canvas.set_drawable(drawable);
        higher_clipregions.push(canvas).unwrap();
    }

    // create a new index map out of the recomputed higher_clipregions
    let mut map: FnvIndexMap<Gid, Canvas, U32> = FnvIndexMap::new();
    for &c in higher_clipregions.iter() {
        map.insert(c.gid(), c).unwrap();
    }

    map
}

// Crate-level draw_textview() call for "local" services that can't use the lib.rs API
pub fn draw_textview(gam_cid: xous::CID, tv: &mut TextView) -> Result<(), xous::Error> {
    let mut sendable_tv = Sendable::new(tv).expect("can't create sendable textview");
    sendable_tv.set_op(TextOp::Render);
    sendable_tv.lend_mut(gam_cid, sendable_tv.get_op().into()).expect("draw_textview operation failure");

    sendable_tv.set_op(TextOp::Nop);
    Ok(())
}
