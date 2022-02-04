use std::collections::HashMap;
use std::collections::BinaryHeap;

use core::cmp::Ordering;
use graphics_server::*;
use log::{error, info};

// "Drawable" vs "NotDrawable" is a security distinction.
// "OnScreen" vs "Offscreen" is a layout distinction.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum CanvasState {
    // this state indicates the Canvas is on-screen and can be drawn, and may or may not need to be flushed to the screen.
    DrawableDirty,
    // this state indicates the Canvas is on-screen and has been flushed to the screen.
    DrawableDrawn,
    // indicates that the Canvas is on-screen but not drawable, but needs to be defaced
    NotDrawableDirty,
    // indicates that the Canvas is on-screen not drawable, and has been defaced
    NotDrawableDefaced,
    // indicates that the Canvas was drawable, but is now off-screen and should not be drawn or considered for any computations
    OffScreenDrawable,
    // indicates that the Canvas was not drawable, but is now off-screen and shouldn to be drawn or considered for any computations
    OffScreenNotDrawable,
}

/// A rectangular region that defines a top-left zero relative offset for graphical items
/// and a bottom-right point that defines a clipping area for things drawn inside.
#[derive(Debug, Copy, Clone)]
pub struct Canvas {
    // unique, random identifier for the Canvas
    gid: Gid,
    // screen coordinates of the clipping region
    clip_rect: Rectangle,
    // trust level, 255 is most trusted
    trust_level: u8,
    // enables scroll/pan of objects within a region
    pan_offset: Point,
    // track the drawing state of the canvas
    state: CanvasState,
    // The type of canvas. Useful for debugging, don't remove it.
    #[allow(dead_code)]
    canvas_type: crate::api::CanvasType,
}

#[allow(dead_code)]
impl Canvas {
    pub fn new(clip_rect: Rectangle, trust_level: u8,
        trng: &trng::Trng, pan_offset: Option<Point>, canvas_type: crate::api::CanvasType) -> Result<Canvas, xous::Error> {

        let mut gid: [u32; 4] = [0; 4];
        let g: u64 = trng.get_u64()?;
        gid[0] = g as u32;
        gid[1] = (g >> 32) as u32;
        let g: u64 = trng.get_u64()?;
        gid[2] = g as u32;
        gid[3] = (g >> 32) as u32;

        Ok(if pan_offset.is_some() {
            Canvas {
                clip_rect, trust_level, state: CanvasState::OffScreenDrawable, gid: Gid::new(gid), pan_offset: pan_offset.unwrap(),
                canvas_type,
            }
        } else {
            Canvas {
                clip_rect, trust_level, state: CanvasState::OffScreenDrawable, gid: Gid::new(gid), pan_offset: Point::new(0, 0),
                canvas_type,
            }
        })
    }
    pub fn intersects(&self, other: &Canvas) -> bool {
        self.clip_rect.intersects(other.clip_rect())
    }
    pub fn less_trusted_than(&self, other: &Canvas) -> bool {
        self.trust_level() < other.trust_level()
    }
    pub fn pan_offset(&self) -> Point { self.pan_offset }
    pub fn clip_rect(&self) -> Rectangle { self.clip_rect }
    pub fn set_clip(&mut self, cr: Rectangle) { self.clip_rect = cr; self.state = CanvasState::OffScreenDrawable }
    pub fn gid(&self) -> Gid { self.gid }
    pub fn trust_level(&self) -> u8 { self.trust_level }
    pub fn set_trust_level(&mut self, level: u8) {self.trust_level = level;}
    pub fn state(&self) -> CanvasState { self.state }
    pub fn is_onscreen(&self) -> bool {
        if self.state == CanvasState::OffScreenDrawable || self.state == CanvasState::OffScreenNotDrawable {
            false
        } else {
            true
        }
    }
    pub fn is_drawable(&self) -> bool {
        if self.state == CanvasState::DrawableDirty || self.state == CanvasState::DrawableDrawn || self.state == CanvasState::OffScreenDrawable {
            true
        } else {
            false
        }
    }
    pub fn is_drawable_or_offscreen(&self) -> bool {
        if self.state == CanvasState::DrawableDirty || self.state == CanvasState::DrawableDrawn
        || self.state == CanvasState::OffScreenDrawable {
            true
        } else {
            false
        }
    }
    pub fn set_drawable(&mut self, drawable: bool) {
        if drawable {
            if self.state == CanvasState::OffScreenNotDrawable {
                self.state = CanvasState::OffScreenDrawable
            } else if self.state != CanvasState::DrawableDrawn {
                self.state = CanvasState::DrawableDirty;
            }
        } else {
            if self.state == CanvasState::OffScreenDrawable {
                self.state = CanvasState::OffScreenNotDrawable
            } else if self.state != CanvasState::NotDrawableDefaced {
                self.state = CanvasState::NotDrawableDirty;
            }
        }
    }
    pub fn set_onscreen(&mut self, onscreen: bool) {
        if onscreen {
            if self.state == CanvasState::OffScreenDrawable {
                self.state = CanvasState::DrawableDirty;
            } else if self.state == CanvasState::OffScreenNotDrawable {
                self.state = CanvasState::NotDrawableDirty;
            }
            // other states are already onscreen
        } else {
            if self.state == CanvasState::DrawableDirty || self.state == CanvasState::DrawableDrawn {
                self.state = CanvasState::OffScreenDrawable;
            } else if self.state == CanvasState::NotDrawableDirty || self.state == CanvasState::NotDrawableDefaced {
                self.state = CanvasState::OffScreenNotDrawable;
            }
            // other states are already offscreen
        }
    }
    // call this after the screen has been flushed
    pub fn do_flushed(&mut self) -> Result<(), xous::Error> {
        if self.state == CanvasState::DrawableDirty || self.state == CanvasState::DrawableDrawn {
            self.state = CanvasState::DrawableDrawn;
            Ok(())
        } else if self.state == CanvasState::NotDrawableDefaced
        || self.state == CanvasState::OffScreenNotDrawable
        || self.state == CanvasState::OffScreenDrawable {
            Ok(())
        } else {
            error!("Canvas: flush happened before not drawable regions were defaced, or before initialized! {:?}", self.state);
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
    pub fn needs_defacing(&self) -> bool {
        if self.state == CanvasState::NotDrawableDirty {
            true
        } else {
            false
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


pub fn deface(gfx: &graphics_server::Gfx, trng: &trng::Trng, canvases: &mut HashMap<Gid, Canvas>) -> bool {
    // first check if any need defacing, if not, then we're done
    let mut needs_defacing = false;
    let mut defaced = false;  // this is set if any drawing actually happens
    for (_, c) in canvases.iter() {
        if c.needs_defacing() {
            needs_defacing = true;
        }
    }
    if needs_defacing {
        log::debug!("doing a deface computation!");

        let screensize = gfx.screen_size().unwrap();
        let screen_rect = Rectangle::new(Point::new(0, 0,), screensize);
        /*
        This routine will need to do something similar to recompute_canvases, where it extracts
        a sorted order and draws the defacement upon the canvas that requires defacing.

        For simplicity, we may be able to assume this is called with at most one layout change
        in between states, so in the worst case we are drawing defacement with a rectangular clip
        area open in the middle of a canvas...
         */
        for (_, c) in canvases.iter_mut() {
            if c.needs_defacing() {
                let clip_rect = c.clip_rect();
                if clip_rect.intersects(screen_rect) {
                    let width = clip_rect.br().x - clip_rect.tl().x;
                    let height = clip_rect.br().y - clip_rect.tl().y;

                    // roughly scale the number of lines hatched by the size of the clipping area to deface
                    let mut num_lines = (width + height) / 24;
                    if num_lines < 8 {
                        num_lines = 8;
                    }
                    if num_lines > 40 {
                        num_lines = 40;
                    }
                    // log::debug!("deface width {} height {}, numlines {}, cliprect {:?}", width, height, num_lines, clip_rect);

                    // draw 32 lines, of random orientation and lengths, across the clip area.
                    for _ in 0..num_lines {
                        // do the actual defacing
                        // get 64 bits of entropy, and express it geometrically
                        let mut rand = trng.get_u64().unwrap();
                        let mut x = (rand & 0xFFF) as i16;
                        rand >>= 12;
                        let mut y = (rand & 0xFFF) as i16;
                        rand >>= 12;
                        let mut delta_x = (rand & 0x3F) as i16 + 64;
                        rand >>= 6;
                        if (rand & 1) == 1 {
                            delta_x = -delta_x;
                        }
                        rand >>= 1;
                        let mut delta_y = (rand & 0x3F) as i16 + 64;
                        rand >>= 6;
                        if (rand & 1) == 1 {
                            delta_y = -delta_y;
                        }
                        // rand >>= 1;

                        x = remap_rand(width as _, x as _, 0xfff);
                        y = remap_rand(height as _, y as _, 0xfff);

                        gfx.draw_line_clipped_xor(
                            Line::new_with_style(
                                Point::new(x, y),
                                Point::new(x + delta_x, y + delta_y),
                                DrawStyle::new(PixelColor::Dark, PixelColor::Dark, 1)),
                                clip_rect).unwrap();
                    }
                    defaced = true;
                }

                // indicate that the defacement has happened to the canvas state machine
                c.do_defaced().expect("couldn't update defacement state");
            }
        }
    }
    defaced
}
fn remap_rand(end_range: i32, rand: i16, source_range: i32) -> i16 {
    // x is a number from 0 through 2^12 -1 = 4095
    // we want to take 0<->4095 and renormalize to a range of -width/2 <-> width/2 and
    // then add it to width/2 to get a somewhat regular distribution (a modulus would
    // tend to bias lines "away" from the lower and right edges since both points of
    // the line would have to, by chance, be just up to the width but not larger than that)
    (
        (((end_range as i32 * rand as i32 * 100i32) / source_range) // remap to 0:end_range*100
        - (end_range as i32 * 100i32) / 2i32) // remap to -end_range*100/2:end_range*100/2
        / 100i32 // remap to -end_range/2:end_range/2
        + (end_range as i32 / 2) // remap to 0:end_range
    ) as i16
}

// we use the "screen" parameter to determine when we can turn off drawing to canvases that are off-screen
pub fn recompute_canvases(canvases: &HashMap<Gid, Canvas>, screen: Rectangle) -> HashMap<Gid, Canvas> {
    let debug = false; // keep this around, it's really convenient for turning off just this specific subset of debug messages
    // first, sort canvases by trust_level. Canvas implements ord/eq based on the trust_level attribute
    // so jush pushing it into a max binary heap does the trick.
    if debug { info!("CANVAS: recompute canvas"); }
    let mut sorted_clipregions: BinaryHeap<Canvas> = BinaryHeap::new();
    for (&k, &c) in canvases.iter() {
        if debug { info!("   CANVAS: sorting gid {:?}, canvas {:?}", k, c);}
        sorted_clipregions.push(c); // always succeeds because incoming type is the same size
    }

    // now, descend through trust levels and compute intersections, putting the updated drawable states into higher_clipregions
    let mut higher_clipregions: BinaryHeap<&Canvas> = BinaryHeap::new();
    let mut current_trust_level: u8 = 255;
    // sorted_clipregions is a Max heap keyed on trust, so popping the elements off will return them sorted from most to least trusted
    if debug{info!("CANVAS: received screen argument of {:?}", screen);}
    if debug{info!("CANVAS: now determining which regions are drawable");}

    let mut sorted_clipregions_vec = sorted_clipregions.into_sorted_vec();
    for canvas in sorted_clipregions_vec.iter_mut() {
        if canvas.is_onscreen() {
            if canvas.trust_level() < current_trust_level {
                current_trust_level = canvas.trust_level();
            }
            for region in higher_clipregions.iter() {
                if region.is_onscreen()
                && (region.trust_level() > current_trust_level) // regions of same trust level have arbitrary draw order
                && region.clip_rect().intersects(canvas.clip_rect()) {
                    canvas.set_drawable(false);
                } else {
                    canvas.set_drawable(true);
                }
            }
        }
        higher_clipregions.push(canvas);
    }

    // create a new index map out of the recomputed higher_clipregions
    let mut map: HashMap<Gid, Canvas> = HashMap::new();
    if debug { info!("CANVAS: reconstituting index map");}
    for &c in higher_clipregions.iter() {
        if debug { info!("   CANVAS: inserting gid {:?}, canvas {:?}", c.gid(), c);}
        map.insert(c.gid(), *c);
    }

    map
}
