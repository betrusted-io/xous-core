#![cfg_attr(target_os = "none", no_std)]

// pub mod size;
pub mod api;
#[cfg(feature = "ditherpunk")]
pub use api::Tile;
pub use api::{
    Circle, ClipObject, ClipObjectList, ClipObjectType, ClipRect, Cursor, DrawStyle, Gid, GlyphStyle, Line,
    PixelColor, Point, Rectangle, RoundedRectangle, TextBounds, TextOp, TextView, TokenClaim,
};
pub mod op;

pub mod fontmap;
pub use api::ArchivedBulkRead;
pub use api::BulkRead;
use api::{Opcode, TEXTVIEW_LEN}; // if you prefer to map the api into your local namespace
pub use fontmap::*;
use num_traits::ToPrimitive;
use xous::{Message, send_message};
use xous_ipc::Buffer;
#[derive(Debug)]
pub struct Gfx {
    conn: xous::CID,
}
impl Gfx {
    pub fn new(xns: &xous_names::XousNames) -> Result<Self, xous::Error> {
        REFCOUNT.fetch_add(1, Ordering::Relaxed);
        let conn = xns.request_connection_blocking(api::SERVER_NAME_GFX).expect("Can't connect to GFX");
        Ok(Gfx { conn })
    }

    pub fn conn(&self) -> xous::CID { self.conn }

    /// Draws a line on the graphics server.
    ///
    /// # Example
    /// ```
    /// use graphics_server::{Gfx, Line, LineStyle, Point};
    /// let gfx = Gfx::new(&xous_names::XousNames::new().unwrap()).unwrap();
    /// let line = Line::new(Point::new(0, 0), Point::new(100, 100), LineStyle::Solid);
    /// gfx.draw_line(line).unwrap();
    /// ```
    pub fn draw_line(&self, line: Line) -> Result<(), xous::Error> {
        send_message(
            self.conn,
            Message::new_scalar(
                Opcode::Line.to_usize().unwrap(),
                line.start.into(),
                line.end.into(),
                line.style.into(),
                0,
            ),
        )
        .map(|_| ())
    }

    /// Draws a circle on the graphics server.
    ///
    /// # Example
    /// ```
    /// use graphics_server::{Circle, Gfx, LineStyle, Point};
    /// let gfx = Gfx::new(&xous_names::XousNames::new().unwrap()).unwrap();
    /// let circle = Circle::new(Point::new(50, 50), 25, LineStyle::Solid);
    /// gfx.draw_circle(circle).unwrap();
    /// ```
    pub fn draw_circle(&self, circ: Circle) -> Result<(), xous::Error> {
        send_message(
            self.conn,
            Message::new_scalar(
                Opcode::Circle.to_usize().unwrap(),
                circ.center.into(),
                circ.radius as usize,
                circ.style.into(),
                0,
            ),
        )
        .map(|_| ())
    }

    /// Draws a rectangle on the graphics server.
    ///
    /// # Example
    /// ```
    /// use graphics_server::{Gfx, LineStyle, Point, Rectangle};
    /// let gfx = Gfx::new(&xous_names::XousNames::new().unwrap()).unwrap();
    /// let rect = Rectangle::new(Point::new(10, 10), Point::new(50, 50), LineStyle::Solid);
    /// gfx.draw_rectangle(rect).unwrap();
    /// ```
    pub fn draw_rectangle(&self, rect: Rectangle) -> Result<(), xous::Error> {
        send_message(
            self.conn,
            Message::new_scalar(
                Opcode::Rectangle.to_usize().unwrap(),
                rect.tl.into(),
                rect.br.into(),
                rect.style.into(),
                0,
            ),
        )
        .map(|_| ())
    }

    /// Draws a rounded rectangle on the graphics server.
    ///
    /// # Example
    /// ```
    /// use graphics_server::{Gfx, LineStyle, Point, Rectangle, RoundedRectangle};
    /// let gfx = Gfx::new(&xous_names::XousNames::new().unwrap()).unwrap();
    /// let rr = RoundedRectangle::new(
    ///     Rectangle::new(Point::new(10, 10), Point::new(50, 50), LineStyle::Solid),
    ///     5,
    /// );
    /// gfx.draw_rounded_rectangle(rr).unwrap();
    /// ```
    pub fn draw_rounded_rectangle(&self, rr: RoundedRectangle) -> Result<(), xous::Error> {
        send_message(
            self.conn,
            Message::new_scalar(
                Opcode::RoundedRectangle.to_usize().unwrap(),
                rr.border.tl.into(),
                rr.border.br.into(),
                rr.border.style.into(),
                rr.radius as _,
            ),
        )
        .map(|_| ())
    }

    /// Flushes the graphics server, ensuring all previous drawing commands are executed.
    ///
    /// # Example
    /// ```
    /// use graphics_server::Gfx;
    /// let gfx = Gfx::new(&xous_names::XousNames::new().unwrap()).unwrap();
    /// gfx.flush().unwrap();
    /// ```
    pub fn flush(&self) -> Result<(), xous::Error> {
        send_message(self.conn, Message::new_scalar(Opcode::Flush.to_usize().unwrap(), 0, 0, 0, 0))
            .map(|_| ())
    }

    /// Draws the sleep screen on the graphics server.
    ///
    /// This function sends a message to the graphics server to draw the sleep screen.
    ///
    /// # Example
    /// ```
    /// use graphics_server::Gfx;
    /// let gfx = Gfx::new(&xous_names::XousNames::new().unwrap()).unwrap();
    /// gfx.draw_sleepscreen().unwrap();
    /// ```
    pub fn draw_sleepscreen(&self) -> Result<(), xous::Error> {
        send_message(self.conn, Message::new_scalar(Opcode::DrawSleepScreen.to_usize().unwrap(), 0, 0, 0, 0))
            .map(|_| ())
    }

    /// Draws the boot logo on the graphics server.
    ///
    /// This function sends a message to the graphics server to draw the boot logo.
    /// The boot logo is typically displayed during the device's startup sequence.
    ///
    /// # Example
    /// ```
    /// use graphics_server::Gfx;
    /// let gfx = Gfx::new(&xous_names::XousNames::new().unwrap()).unwrap();
    /// gfx.draw_boot_logo().unwrap();
    /// ```
    pub fn draw_boot_logo(&self) -> Result<(), xous::Error> {
        send_message(self.conn, Message::new_scalar(Opcode::DrawBootLogo.to_usize().unwrap(), 0, 0, 0, 0))
            .map(|_| ())
    }

    /// Retrieves the screen size from the graphics server.
    ///
    /// This function sends a message to the graphics server to query the screen size.
    /// The screen size is returned as a `Point` representing the width and height.
    ///
    /// # Example
    /// ```
    /// use graphics_server::Gfx;
    /// let gfx = Gfx::new(&xous_names::XousNames::new().unwrap()).unwrap();
    /// let screen_dimensions = gfx.screen_size().unwrap();
    /// println!("Screen size: {}x{}", screen_size.x, screen_size.y);
    /// ```
    pub fn screen_size(&self) -> Result<Point, xous::Error> {
        let response = send_message(
            self.conn,
            Message::new_blocking_scalar(Opcode::ScreenSize.to_usize().unwrap(), 0, 0, 0, 0),
        )
        .expect("ScreenSize message failed");
        if let xous::Result::Scalar2(x, y) = response {
            Ok(Point::new(x as _, y as _))
        } else {
            panic!("unexpected return value: {:#?}", response);
        }
    }

    /// Retrieves the height hint for a specific glyph from the graphics server.
    ///
    /// This function sends a message to the graphics server to query the height hint of a glyph.
    /// The height hint is returned as a `usize`.
    ///
    /// # Example
    /// ```
    /// use graphics_server::{Gfx, GlyphStyle};
    /// let gfx = Gfx::new(&xous_names::XousNames::new().unwrap()).unwrap();
    /// let height_hint = gfx.glyph_height_hint(GlyphStyle::Regular).unwrap();
    /// println!("Glyph height hint: {}", height_hint);
    /// ```
    pub fn glyph_height_hint(&self, glyph: GlyphStyle) -> Result<usize, xous::Error> {
        let response = send_message(
            self.conn,
            Message::new_blocking_scalar(
                Opcode::QueryGlyphProps.to_usize().unwrap(),
                glyph as usize,
                0,
                0,
                0,
            ),
        )
        .expect("QueryGlyphProps failed");
        if let xous::Result::Scalar2(_, h) = response {
            Ok(h)
        } else {
            panic!("unexpected return value: {:#?}", response);
        }
    }

    /// Draws a `TextView` on the graphics server.
    ///
    /// This function sends a message to the graphics server to draw the specified `TextView`.
    /// If the text in the `TextView` is too long to transmit in a single page of memory, it will be
    /// truncated.
    ///
    /// # Example
    /// ```
    /// use graphics_server::{Gfx, TextView};
    /// let mut tv = TextView::new();
    /// tv.set_text("Hello, world!");
    /// let gfx = Gfx::new(&xous_names::XousNames::new().unwrap()).unwrap();
    /// gfx.draw_textview(&mut tv).unwrap();
    /// ````
    pub fn draw_textview(&self, tv: &mut TextView) -> Result<(), xous::Error> {
        if tv.text.len() > TEXTVIEW_LEN {
            tv.text.truncate(TEXTVIEW_LEN);
        }
        let mut buf = Buffer::into_buf(tv.clone()).or(Err(xous::Error::InternalError))?;
        buf.lend_mut(self.conn, Opcode::DrawTextView.to_u32().unwrap())
            .or(Err(xous::Error::InternalError))?;

        let tvr = buf.to_original::<TextView, _>().unwrap();
        tv.bounds_computed = tvr.bounds_computed;
        tv.cursor = tvr.cursor;
        tv.overflow = tvr.overflow;
        tv.busy_animation_state = tvr.busy_animation_state;
        Ok(())
    }

    /// Draws a line on the graphics server with clipping.
    ///
    /// This function sends a message to the graphics server to draw the specified `Line` within the specified
    /// `Rectangle` clip area.
    ///
    /// # Example
    /// ```
    /// use graphics_server::{Gfx, Line, LineStyle, Point, Rectangle};
    /// let line = Line::new(Point::new(0, 0), Point::new(100, 100), LineStyle::Solid);
    /// let clip = Rectangle::new(Point::new(10, 10), Point::new(90, 90), LineStyle::Solid);
    /// let gfx = Gfx::new(&xous_names::XousNames::new().unwrap()).unwrap();
    /// gfx.draw_line_clipped(line, clip).unwrap();
    /// ```
    pub fn draw_line_clipped(&self, line: Line, clip: Rectangle) -> Result<(), xous::Error> {
        let co = ClipObject { clip, obj: ClipObjectType::Line(line) };
        let buf = Buffer::into_buf(co).or(Err(xous::Error::InternalError))?;
        buf.lend(self.conn, Opcode::DrawClipObject.to_u32().unwrap()).map(|_| ())
    }

    /// Draws a line on the graphics server with XOR clipping.
    /// For use in the deface operation.
    ///
    /// This function sends a message to the graphics server to draw the specified `Line` within the specified
    /// `Rectangle` clip area using XOR mode.
    ///
    /// # Example
    /// ```
    /// use graphics_server::{Gfx, Line, LineStyle, Point, Rectangle};
    /// let line = Line::new(Point::new(0, 0), Point::new(100, 100), LineStyle::Solid);
    /// let clip = Rectangle::new(Point::new(10, 10), Point::new(90, 90), LineStyle::Solid);
    /// let gfx = Gfx::new(&xous_names::XousNames::new().unwrap()).unwrap();
    /// gfx.draw_line_clipped_xor(line, clip).unwrap();
    /// ```
    pub fn draw_line_clipped_xor(&self, line: Line, clip: Rectangle) -> Result<(), xous::Error> {
        let co = ClipObject { clip, obj: ClipObjectType::XorLine(line) };
        let buf = Buffer::into_buf(co).or(Err(xous::Error::InternalError))?;
        buf.lend(self.conn, Opcode::DrawClipObject.to_u32().unwrap()).map(|_| ())
    }

    /// Draws a circle on the graphics server with clipping.
    ///
    /// This function sends a message to the graphics server to draw the specified `Circle` within the
    /// specified `Rectangle` clip area.
    ///
    /// # Example
    /// ```
    /// use graphics_server::{Circle, Gfx, LineStyle, Point, Rectangle};
    /// let circ = Circle::new(Point::new(50, 50), 25, LineStyle::Solid);
    /// let clip = Rectangle::new(Point::new(10, 10), Point::new(90, 90), LineStyle::Solid);
    /// let gfx = Gfx::new(&xous_names::XousNames::new().unwrap()).unwrap();
    /// gfx.draw_circle_clipped(circ, clip).expect("Failed to draw clipped circle");
    ///     Ok(_) => println!("Circle clipped successfully"),
    ///     Err(e) => eprintln!("Failed to clip circle: {:?}", e),
    /// }
    /// ```
    pub fn draw_circle_clipped(&self, circ: Circle, clip: Rectangle) -> Result<(), xous::Error> {
        let co = ClipObject { clip, obj: ClipObjectType::Circ(circ) };
        let buf = Buffer::into_buf(co).or(Err(xous::Error::InternalError))?;
        buf.lend(self.conn, Opcode::DrawClipObject.to_u32().unwrap()).map(|_| ())
    }

    /// Draws a rectangle on the graphics server with clipping.
    ///
    /// This function sends a message to the graphics server to draw the specified `Rectangle`
    /// within the specified `ClipRect` clip area.
    ///
    /// # Example
    /// ```
    /// use graphics_server::{Gfx, LineStyle, Point, Rectangle};
    /// let rect = Rectangle::new(Point::new(10, 10), Point::new(50, 50), LineStyle::Solid);
    /// let clip = Rectangle::new(Point::new(0, 0), Point::new(100, 100), LineStyle::Solid);
    /// match gfx.draw_rectangle_clipped(rect, clip) {
    ///     Ok(_) => println!("Rectangle clipped successfully"),
    ///     Err(e) => eprintln!("Failed to clip rectangle: {:?}", e),
    /// }
    /// ```
    pub fn draw_rectangle_clipped(&self, rect: Rectangle, clip: Rectangle) -> Result<(), xous::Error> {
        let co = ClipObject { clip, obj: ClipObjectType::Rect(rect) };
        let buf = Buffer::into_buf(co).or(Err(xous::Error::InternalError))?;
        buf.lend(self.conn, Opcode::DrawClipObject.to_u32().unwrap()).map(|_| ())
    }

    /// Draws a rounded rectangle on the graphics server with clipping.
    ///
    /// This function sends a message to the graphics server to draw the specified `RoundedRectangle`
    /// within the specified `ClipRect` clip area.
    ///
    /// # Example
    /// ```
    /// use graphics_server::{Gfx, LineStyle, Point, Rectangle, RoundedRectangle};
    /// let rr = RoundedRectangle::new(
    ///     Rectangle::new(Point::new(10, 10), Point::new(50, 50), LineStyle::Solid),
    ///     5,
    /// );
    /// let clip = Rectangle::new(Point::new(0, 0), Point::new(100, 100), LineStyle::Solid);
    /// gfx.draw_rounded_rectangle_clipped(rr, clip).unwrap();
    /// ```
    pub fn draw_rounded_rectangle_clipped(
        &self,
        rr: RoundedRectangle,
        clip: Rectangle,
    ) -> Result<(), xous::Error> {
        let co = ClipObject { clip, obj: ClipObjectType::RoundRect(rr) };
        let buf = Buffer::into_buf(co).or(Err(xous::Error::InternalError))?;
        buf.lend(self.conn, Opcode::DrawClipObject.to_u32().unwrap()).map(|_| ())
    }

    /// Draws a tile on the graphics server with clipping.
    ///
    /// This function sends a message to the graphics server to draw the specified `Tile`
    /// within the specified `ClipRect` clip area.
    ///
    /// # Example
    /// ```
    /// use graphics_server::{Gfx, Point, Rectangle, Tile};
    /// let tile = Tile::new(Point::new(10, 10), Point::new(50, 50));
    /// let clip = Rectangle::new(Point::new(0, 0), Point::new(100, 100), LineStyle::Solid);
    /// gfx.draw_tile_clipped(tile, clip).unwrap();
    /// ```
    #[cfg(feature = "ditherpunk")]
    pub fn draw_tile_clipped(&self, tile: Tile, clip: Rectangle) -> Result<(), xous::Error> {
        let co = ClipObject { clip, obj: ClipObjectType::Tile(tile) };
        log::info!("ClipObject size: {}", core::mem::size_of::<ClipObject>());
        let buf = Buffer::into_buf(co).or(Err(xous::Error::InternalError))?;
        buf.lend(self.conn, Opcode::DrawClipObject.to_u32().unwrap()).map(|_| ())
    }

    /// Draws a list of objects on the graphics server with clipping.
    ///
    /// This function sends a message to the graphics server to draw the specified list of objects
    /// within the specified `ClipRect` clip area.
    ///
    /// # Example
    /// ```
    /// use graphics_server::{ClipObjectList, Gfx, Point, Rectangle};
    /// let object_list = ClipObjectList::default();
    /// let gfx = Gfx::new(&xous_names::XousNames::new().unwrap()).unwrap();
    /// gfx.draw_object_list_clipped(object_list).unwrap();
    /// ```
    pub fn draw_object_list_clipped(&self, list: ClipObjectList) -> Result<(), xous::Error> {
        let buf = Buffer::into_buf(list).or(Err(xous::Error::InternalError))?;
        buf.lend(self.conn, Opcode::DrawClipObjectList.to_u32().unwrap()).map(|_| ())
    }

    /// Sets the developer boot mode on the graphics server.
    ///
    /// This function sends a message to the graphics server to enable or disable the developer boot mode.
    /// Once you've set it, you can't unset it.
    ///
    /// # Example
    /// ```
    /// use graphics_server::Gfx;
    /// let gfx = Gfx::new(&xous_names::XousNames::new().unwrap()).unwrap();
    /// gfx.set_devboot(true).unwrap(); // Enable developer boot mode
    /// gfx.set_devboot(false).unwrap(); // Disable developer boot mode
    /// ```
    pub fn set_devboot(&self, enable: bool) -> Result<(), xous::Error> {
        let ena = if enable { 1 } else { 0 };
        send_message(self.conn, Message::new_scalar(Opcode::Devboot.to_usize().unwrap(), ena, 0, 0, 0))
            .map(|_| ())
    }

    /// Reads the font map in bulk from the graphics server.
    ///
    /// Instead of implementing the read in the library, we hand the raw opcode to the caller.
    /// This allows the caller to re-use the bulk read data structure across multiple reads
    /// instead of it being re-allocated and re-initialized every single call.
    ///
    /// # Example
    /// ```
    /// use graphics_server::{ArchivedBulkRead, Gfx};
    /// let gfx = Gfx::new(&xous_names::XousNames::new().unwrap()).unwrap();
    /// let font_map = gfx.bulk_read_fontmap_op(0).unwrap();
    /// // Process the font map as needed
    /// ```
    pub fn bulk_read_fontmap_op(&self) -> u32 { Opcode::BulkReadFonts.to_u32().unwrap() }

    /// Resets the bulk read pointer on the graphics server.
    ///
    /// The bulk read operation auto-increments a pointer on the graphics server, so this message is necessary
    /// to reset the pointer to 0.
    ///
    /// # Example
    /// ```
    /// use graphics_server::Gfx;
    /// let gfx = Gfx::new(&xous_names::XousNames::new().unwrap()).unwrap();
    /// gfx.bulk_read_restart().unwrap();
    /// ```
    pub fn bulk_read_restart(&self) {
        send_message(
            self.conn,
            Message::new_blocking_scalar(Opcode::RestartBulkRead.to_usize().unwrap(), 0, 0, 0, 0),
        )
        .expect("couldn't reset bulk read");
    }

    /// Runs a self-test pattern on the graphics server.
    ///
    /// This function sends a message to the graphics server to display a test pattern for a specified
    /// duration. The test pattern is typically used to verify the functionality of the graphics hardware.
    ///
    /// # Example
    /// ```
    /// use graphics_server::Gfx;
    /// let gfx = Gfx::new(&xous_names::XousNames::new().unwrap()).unwrap();
    /// gfx.selftest(1000); // Display the test pattern for 1000 milliseconds
    /// ```
    pub fn selftest(&self, duration_ms: usize) {
        send_message(
            self.conn,
            Message::new_blocking_scalar(Opcode::TestPattern.to_usize().unwrap(), duration_ms, 0, 0, 0),
        )
        .expect("couldn't self test");
    }

    /// Stashes the current graphics state on the graphics server.
    ///
    /// This function sends a message to the graphics server to stash (save) the current graphics state.
    /// The stashed state can be restored later using the `pop` function. This is useful for temporarily
    /// saving the current state before making changes, and then restoring it later.
    ///
    /// # Example
    /// ```
    /// use graphics_server::Gfx;
    /// let gfx = Gfx::new(&xous_names::XousNames::new().unwrap()).unwrap();
    /// gfx.stash(true); // Stash the current state in a blocking manner
    /// gfx.stash(false); // Stash the current state in a non-blocking manner
    /// ```
    pub fn stash(&self, blocking: bool) {
        if blocking {
            send_message(
                self.conn,
                Message::new_blocking_scalar(Opcode::Stash.to_usize().unwrap(), 0, 0, 0, 0),
            )
            .expect("couldn't stash");
        } else {
            send_message(self.conn, Message::new_scalar(Opcode::Stash.to_usize().unwrap(), 0, 0, 0, 0))
                .expect("couldn't stash");
        }
    }

    /// Restores the previously stashed graphics state on the graphics server.
    ///
    /// This function sends a message to the graphics server to restore the previously stashed graphics state.
    /// The stashed state can be restored using this function after it has been saved using the `stash`
    /// function. This is useful for reverting to a previous state after making temporary changes.
    ///
    /// # Example
    /// ```
    /// use graphics_server::Gfx;
    /// let gfx = Gfx::new(&xous_names::XousNames::new().unwrap()).unwrap();
    /// gfx.stash(true); // Stash the current state in a blocking manner
    /// // Make some changes to the graphics state
    /// gfx.pop(true); // Restore the previously stashed state in a blocking manner
    /// gfx.stash(false); // Stash the current state in a non-blocking manner
    /// // Make some changes to the graphics state
    /// gfx.pop(false); // Restore the previously stashed state in a non-blocking manner
    /// ```
    pub fn pop(&self, blocking: bool) {
        if blocking {
            send_message(
                self.conn,
                Message::new_blocking_scalar(Opcode::Pop.to_usize().unwrap(), 0, 0, 0, 0),
            )
            .expect("couldn't pop");
        } else {
            send_message(self.conn, Message::new_scalar(Opcode::Pop.to_usize().unwrap(), 0, 0, 0, 0))
                .expect("couldn't pop");
        }
    }
}

use core::sync::atomic::{AtomicU32, Ordering};
static REFCOUNT: AtomicU32 = AtomicU32::new(0);
impl Drop for Gfx {
    fn drop(&mut self) {
        // de-allocate myself. It's unsafe because we are responsible to make sure nobody else is using the
        // connection.
        if REFCOUNT.fetch_sub(1, Ordering::Relaxed) == 1 {
            unsafe {
                xous::disconnect(self.conn).unwrap();
            }
        }
    }
}
