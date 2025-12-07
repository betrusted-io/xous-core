use blitstr2::GlyphStyle;
use num_traits::ToPrimitive;
use xous::{Message, send_message};
use xous_ipc::Buffer;

#[cfg(feature = "ditherpunk")]
use crate::minigfx::Tile;
use crate::minigfx::*;
use crate::service::api::*;
#[derive(Debug)]
pub struct Gfx {
    conn: xous::CID,
}

impl Clone for Gfx {
    fn clone(&self) -> Self {
        REFCOUNT.fetch_add(1, Ordering::Relaxed);
        Gfx { conn: self.conn }
    }
}

impl Gfx {
    pub fn new(xns: &xous_names::XousNames) -> Result<Self, xous::Error> {
        REFCOUNT.fetch_add(1, Ordering::Relaxed);
        let conn = xns
            .request_connection_blocking(crate::service::api::SERVER_NAME_GFX)
            .expect("Can't connect to GFX");
        Ok(Gfx { conn })
    }

    pub fn conn(&self) -> xous::CID { self.conn }

    /// Draws a line.
    ///
    /// <details>
    ///     <summary>Example Image</summary>
    ///
    /// ![Example Image](https://github.com/betrusted-io/xous-core/blob/main/docs/images/graphics-server/line.png?raw=true)
    ///
    /// </details>
    ///
    /// # Example
    /// ```
    /// use graphics_server::{Gfx, Line, Point};
    /// let gfx = Gfx::new(&xous_names::XousNames::new().unwrap()).unwrap();
    /// let line = Line::new(Point::new(100, 100), Point::new(200, 200)); // if you want to specify a style, use Line::new_with_style
    /// gfx.draw_line(line).unwrap();
    /// gfx.flush().unwrap();
    /// ```
    pub fn draw_line(&self, line: Line) -> Result<(), xous::Error> {
        send_message(
            self.conn,
            Message::new_scalar(
                GfxOpcode::Line.to_usize().unwrap(),
                line.start.into(),
                line.end.into(),
                line.style.into(),
                0,
            ),
        )
        .map(|_| ())
    }

    /// Draws a circle.
    ///
    /// <details>
    ///     <summary>Example Image</summary>
    ///
    /// ![Example Image](https://github.com/betrusted-io/xous-core/blob/main/docs/images/graphics-server/circle.png?raw=true)
    ///
    /// </details>
    ///
    /// # Example
    /// ```
    /// use graphics_server::{Circle, Gfx, Point};
    /// let gfx = Gfx::new(&xous_names::XousNames::new().unwrap()).unwrap();
    /// let circle = Circle::new(Point::new(150, 150), 25); // if you want to specify a style, use Circle::new_with_style
    /// gfx.draw_circle(circle).unwrap();
    /// gfx.flush().unwrap();
    /// ```
    pub fn draw_circle(&self, circ: Circle) -> Result<(), xous::Error> {
        send_message(
            self.conn,
            Message::new_scalar(
                GfxOpcode::Circle.to_usize().unwrap(),
                circ.center.into(),
                circ.radius as usize,
                circ.style.into(),
                0,
            ),
        )
        .map(|_| ())
    }

    /// Draws a rectangle.
    ///
    /// <details>
    ///     <summary>Example Image</summary>
    ///
    /// ![Example Image](https://github.com/betrusted-io/xous-core/blob/main/docs/images/graphics-server/rectangle.png?raw=true)
    ///
    /// </details>
    ///
    /// # Example
    /// ```
    /// use graphics_server::{Gfx, Point, Rectangle};
    /// let gfx = Gfx::new(&xous_names::XousNames::new().unwrap()).unwrap();
    /// let rect = Rectangle::new(Point::new(100, 100), Point::new(250, 150)); // if you want to specify a style, use Rectangle::new_with_style
    /// gfx.draw_rectangle(rect).unwrap();
    /// gfx.flush().unwrap();
    /// ```
    pub fn draw_rectangle(&self, rect: Rectangle) -> Result<(), xous::Error> {
        send_message(
            self.conn,
            Message::new_scalar(
                GfxOpcode::Rectangle.to_usize().unwrap(),
                rect.tl.into(),
                rect.br.into(),
                rect.style.into(),
                0,
            ),
        )
        .map(|_| ())
    }

    /// Draws a rounded rectangle.
    ///
    /// <details>
    ///     <summary>Example Image</summary>
    ///
    /// ![Example Image](https://github.com/betrusted-io/xous-core/blob/main/docs/images/graphics-server/rounded_rectangle.png?raw=true)
    ///
    /// </details>
    ///
    /// # Example
    /// ```
    /// use graphics_server::{Gfx, Point, Rectangle, RoundedRectangle};
    /// let gfx = Gfx::new(&xous_names::XousNames::new().unwrap()).unwrap();
    /// let rr = RoundedRectangle::new(Rectangle::new(Point::new(100, 100), Point::new(150, 200)), 5);
    /// gfx.draw_rounded_rectangle(rr).unwrap();
    /// gfx.flush().unwrap();
    /// ```
    pub fn draw_rounded_rectangle(&self, rr: RoundedRectangle) -> Result<(), xous::Error> {
        send_message(
            self.conn,
            Message::new_scalar(
                GfxOpcode::RoundedRectangle.to_usize().unwrap(),
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
    /// use graphics_server::{Circle, Gfx, Line, Point};
    /// let gfx = Gfx::new(&xous_names::XousNames::new().unwrap()).unwrap();
    /// let line = Line::new(Point::new(0, 0), Point::new(100, 100));
    /// let circle = Circle::new(Point::new(150, 150), 25);
    /// gfx.draw_line(line).unwrap();
    /// gfx.draw_circle(circle).unwrap();
    /// gfx.flush().unwrap(); // Both the line and the circle will be drawn
    /// ```
    pub fn flush(&self) -> Result<(), xous::Error> {
        send_message(self.conn, Message::new_scalar(GfxOpcode::Flush.to_usize().unwrap(), 0, 0, 0, 0))
            .map(|_| ())
    }

    /// Draws the sleep screen.
    ///
    /// This function sends a message to the graphics server to draw the sleep screen.
    ///
    /// <details>
    ///     <summary>Example Image</summary>
    ///
    /// ![Example Image](https://github.com/betrusted-io/xous-core/blob/main/docs/images/graphics-server/sleepscreen.png?raw=true)
    ///
    /// </details>
    ///
    /// # Example
    /// ```
    /// use graphics_server::Gfx;
    /// let gfx = Gfx::new(&xous_names::XousNames::new().unwrap()).unwrap();
    /// gfx.draw_sleepscreen().unwrap();
    /// gfx.flush().unwrap();
    /// ```
    pub fn draw_sleepscreen(&self) -> Result<(), xous::Error> {
        send_message(
            self.conn,
            Message::new_scalar(GfxOpcode::DrawSleepScreen.to_usize().unwrap(), 0, 0, 0, 0),
        )
        .map(|_| ())
    }

    /// Draws the boot logo.
    ///
    /// This function sends a message to the graphics server to draw the boot logo.
    /// The boot logo is typically displayed during the device's startup sequence.
    ///
    /// <details>
    ///     <summary>Example Image</summary>
    ///
    /// ![Example Image](https://github.com/betrusted-io/xous-core/blob/main/docs/images/graphics-server/boot_logo.png?raw=true)
    ///
    /// </details>
    ///
    /// # Example
    /// ```
    /// use graphics_server::Gfx;
    /// let gfx = Gfx::new(&xous_names::XousNames::new().unwrap()).unwrap();
    /// gfx.draw_boot_logo().unwrap();
    /// gfx.flush().unwrap();
    /// ```
    pub fn draw_boot_logo(&self) -> Result<(), xous::Error> {
        send_message(self.conn, Message::new_scalar(GfxOpcode::DrawBootLogo.to_usize().unwrap(), 0, 0, 0, 0))
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
    /// let screen_dimensions = gfx.screen_size().expect("Couldn't get screen size");
    /// println!("Screen size: {}x{}", screen_dimensions.x, screen_dimensions.y);
    /// ```
    pub fn screen_size(&self) -> Result<Point, xous::Error> {
        let response = send_message(
            self.conn,
            Message::new_blocking_scalar(GfxOpcode::ScreenSize.to_usize().unwrap(), 0, 0, 0, 0),
        )
        .expect("ScreenSize message failed");
        if let xous::Result::Scalar5(_, x, y, _, _) = response {
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
                GfxOpcode::QueryGlyphProps.to_usize().unwrap(),
                glyph as usize,
                0,
                0,
                0,
            ),
        )
        .expect("QueryGlyphProps failed");
        if let xous::Result::Scalar5(_, _, h, _, _) = response {
            Ok(h)
        } else {
            panic!("unexpected return value: {:#?}", response);
        }
    }

    /// Draws a `TextView`.
    ///
    /// This function sends a message to the graphics server to draw the specified `TextView`.
    /// If the text in the `TextView` is too long to transmit in a single page of memory, it will be
    /// truncated.
    /// Text that overflows the bounds of the `TextView` will be clipped.
    ///
    /// <details>
    ///     <summary>Example Image</summary>
    ///
    /// ![Example Image](https://github.com/betrusted-io/xous-core/blob/main/docs/images/graphics-server/textview.png?raw=true)
    ///
    /// </details>
    ///
    /// # Example
    /// ```
    /// use graphics_server::{Gfx, TextView};
    /// let clipping_area = Rectangle::new_coords(50, 50, 290, 450);
    /// let text_bounds = Rectangle::new_coords(10, 10, 240, 400);
    /// let mut tv = TextView::new(Gid::new([0, 0, 0, 0]), TextBounds::BoundingBox(text_bounds));
    /// tv.clip_rect = Some(clipping_area);
    /// write!(tv, "Hello, world!").unwrap();
    /// gfx.draw_textview(&mut tv).unwrap();
    /// gfx.flush().unwrap();
    /// ````
    pub fn draw_textview(&self, tv: &mut TextView) -> Result<(), xous::Error> {
        if tv.text.len() > TEXTVIEW_LEN {
            tv.text.truncate(TEXTVIEW_LEN);
        }
        let mut buf = Buffer::into_buf(tv.clone()).or(Err(xous::Error::InternalError))?;
        buf.lend_mut(self.conn, GfxOpcode::DrawTextView.to_u32().unwrap())
            .or(Err(xous::Error::InternalError))?;

        let tvr = buf.to_original::<TextView, _>().unwrap();
        tv.bounds_computed = tvr.bounds_computed;
        tv.cursor = tvr.cursor;
        tv.overflow = tvr.overflow;
        tv.busy_animation_state = tvr.busy_animation_state;
        Ok(())
    }

    /// Bounds computation does no checks on security since it's a non-drawing operation. While normal drawing
    /// always takes the bounds from the canvas, the caller can specify a clip_rect in this tv, instead of
    /// drawing the clip_rect from the Canvas associated with the tv.
    pub fn bounds_compute_textview(&self, tv: &mut TextView) -> Result<(), xous::Error> {
        let mut tv_query = tv.clone();
        tv_query.set_dry_run(true);
        let mut buf = Buffer::into_buf(tv_query).or(Err(xous::Error::InternalError))?;
        buf.lend_mut(self.conn, GfxOpcode::DrawTextView.to_u32().unwrap())
            .or(Err(xous::Error::InternalError))?;
        let tvr = buf.to_original::<TextView, _>().unwrap();

        tv.cursor = tvr.cursor;
        tv.bounds_computed = tvr.bounds_computed;
        tv.overflow = tvr.overflow;
        // don't update the animation state when just computing the textview bounds
        // tv.busy_animation_state = tvr.busy_animation_state;
        Ok(())
    }

    /// Clear the screen in a device-optimized fashion. The exact background color depends on the device.
    pub fn clear(&self) -> Result<(), xous::Error> {
        send_message(self.conn, Message::new_scalar(GfxOpcode::Clear.to_usize().unwrap(), 0, 0, 0, 0))
            .map(|_| ())
    }

    /// Draws a line with clipping.
    ///
    /// This function sends a message to the graphics server to draw the specified `Line` within the specified
    /// `Rectangle` clip area.
    ///
    /// <details>
    ///     <summary>Example Image - clipped line with clipping area represented by light rectangle.</summary>
    ///
    /// ![Example Image](https://github.com/betrusted-io/xous-core/blob/main/docs/images/graphics-server/line_clipped.png?raw=true)
    ///
    /// </details>
    ///
    /// # Example
    /// ```
    /// use graphics_server::{Gfx, Line, Point, Rectangle};
    /// let gfx = Gfx::new(&xous_names::XousNames::new().unwrap()).unwrap();
    /// let line = Line::new(Point::new(100, 100), Point::new(200, 200));
    /// let clip = Rectangle::new(Point::new(110, 110), Point::new(190, 190));
    /// gfx.draw_line_clipped(line, clip).unwrap();
    /// gfx.flush().unwrap();
    /// ```
    pub fn draw_line_clipped(&self, line: Line, clip: Rectangle) -> Result<(), xous::Error> {
        let co = ClipObject { clip, obj: ClipObjectType::Line(line) };
        let buf = Buffer::into_buf(co).or(Err(xous::Error::InternalError))?;
        buf.lend(self.conn, GfxOpcode::DrawClipObject.to_u32().unwrap()).map(|_| ())
    }

    /// Draws a line with XOR clipping.
    /// For use in the deface operation.
    ///
    /// This function sends a message to the graphics server to draw the specified `Line` within the specified
    /// `Rectangle` clip area using XOR mode.
    ///
    /// <details>
    ///     <summary>Example Image - line clipped by area represented by dark rectangle.  Light inner
    /// rectangle added to show XOR behavior.</summary>
    ///
    /// ![Example Image](https://github.com/betrusted-io/xous-core/blob/main/docs/images/graphics-server/line_clipped_xor.png?raw=true)    ///
    /// </details>
    ///
    /// # Example
    /// ```
    /// use graphics_server::{Gfx, Line, Point, Rectangle};
    /// let gfx = Gfx::new(&xous_names::XousNames::new().unwrap()).unwrap();
    /// let line = Line::new(Point::new(0, 0), Point::new(100, 100));
    /// let clip = Rectangle::new(Point::new(10, 10), Point::new(90, 90));
    /// gfx.draw_line_clipped_xor(line, clip).unwrap();
    /// gfx.flush().unwrap();
    /// ```
    pub fn draw_line_clipped_xor(&self, line: Line, clip: Rectangle) -> Result<(), xous::Error> {
        let co = ClipObject { clip, obj: ClipObjectType::XorLine(line) };
        let buf = Buffer::into_buf(co).or(Err(xous::Error::InternalError))?;
        buf.lend(self.conn, GfxOpcode::DrawClipObject.to_u32().unwrap()).map(|_| ())
    }

    /// Draws a circle with clipping.
    ///
    /// This function sends a message to the graphics server to draw the specified `Circle` within the
    /// specified `Rectangle` clip area.
    ///
    /// <details>
    ///     <summary>Example Image - circle clipped by area represented by light rectangle.</summary>
    ///
    /// ![Example Image](https://github.com/betrusted-io/xous-core/blob/main/docs/images/graphics-server/circle_clipped.png?raw=true)
    ///
    /// </details>
    ///
    /// # Example
    /// ```
    /// use graphics_server::{Circle, Gfx, Point, Rectangle};
    /// let gfx = Gfx::new(&xous_names::XousNames::new().unwrap()).unwrap();
    /// let circ = Circle::new(Point::new(130, 130), 25);
    /// let clip = Rectangle::new(Point::new(110, 110), Point::new(190, 190));
    /// gfx.draw_circle_clipped(circ, clip).unwrap();
    /// gfx.flush().unwrap();
    /// ```
    pub fn draw_circle_clipped(&self, circ: Circle, clip: Rectangle) -> Result<(), xous::Error> {
        let co = ClipObject { clip, obj: ClipObjectType::Circ(circ) };
        let buf = Buffer::into_buf(co).or(Err(xous::Error::InternalError))?;
        buf.lend(self.conn, GfxOpcode::DrawClipObject.to_u32().unwrap()).map(|_| ())
    }

    /// Draws a rectangle with clipping.
    ///
    /// This function sends a message to the graphics server to draw the specified `Rectangle`
    /// within the specified `ClipRect` clip area.
    ///
    /// <details>
    ///     <summary>Example Image - rectangle clipped by area represented by light rectangle.</summary>
    ///
    /// ![Example Image](https://github.com/betrusted-io/xous-core/blob/main/docs/images/graphics-server/rectangle_clipped.png?raw=true)
    ///
    /// </details>
    ///
    /// # Example
    /// ```
    /// use graphics_server::{Gfx, Point, Rectangle};
    /// let gfx = Gfx::new(&xous_names::XousNames::new().unwrap()).unwrap();
    /// let rect = Rectangle::new(Point::new(100, 100), Point::new(160, 160));
    /// let clip = Rectangle::new(Point::new(110, 110), Point::new(190, 190));
    /// gfx.draw_rectangle_clipped(rect, clip).unwrap();
    /// gfx.flush().unwrap();
    /// ```
    pub fn draw_rectangle_clipped(&self, rect: Rectangle, clip: Rectangle) -> Result<(), xous::Error> {
        let co = ClipObject { clip, obj: ClipObjectType::Rect(rect) };
        let buf = Buffer::into_buf(co).or(Err(xous::Error::InternalError))?;
        buf.lend(self.conn, GfxOpcode::DrawClipObject.to_u32().unwrap()).map(|_| ())
    }

    /// Draws a rounded rectangle with clipping.
    ///
    /// This function sends a message to the graphics server to draw the specified `RoundedRectangle`
    /// within the specified `ClipRect` clip area.
    ///
    /// <details>
    ///     <summary>Example Image - rounded rectangle clipped by area represented by light
    /// rectangle.</summary>
    ///
    /// ![Example Image](https://github.com/betrusted-io/xous-core/blob/main/docs/images/graphics-server/rounded_rectangle_clipped.png?raw=true)
    ///
    /// </details>
    ///
    /// # Example
    /// ```
    /// use graphics_server::{Gfx, Point, Rectangle, RoundedRectangle};
    /// let gfx = Gfx::new(&xous_names::XousNames::new().unwrap()).unwrap();
    /// let rr = RoundedRectangle::new(Rectangle::new(Point::new(100, 100), Point::new(160, 160)), 8);
    /// let clip = Rectangle::new(Point::new(0, 0), Point::new(100, 100));
    /// gfx.draw_rounded_rectangle_clipped(rr, clip).unwrap();
    /// gfx.flush().unwrap();
    /// ```
    pub fn draw_rounded_rectangle_clipped(
        &self,
        rr: RoundedRectangle,
        clip: Rectangle,
    ) -> Result<(), xous::Error> {
        let co = ClipObject { clip, obj: ClipObjectType::RoundRect(rr) };
        let buf = Buffer::into_buf(co).or(Err(xous::Error::InternalError))?;
        buf.lend(self.conn, GfxOpcode::DrawClipObject.to_u32().unwrap()).map(|_| ())
    }

    #[cfg(feature = "ditherpunk")]
    pub fn draw_tile_clipped(&self, tile: Tile, clip: Rectangle) -> Result<(), xous::Error> {
        let co = ClipObject { clip, obj: ClipObjectType::Tile(tile) };
        log::info!("ClipObject size: {}", core::mem::size_of::<ClipObject>());
        let buf = Buffer::into_buf(co).or(Err(xous::Error::InternalError))?;
        buf.lend(self.conn, GfxOpcode::DrawClipObject.to_u32().unwrap()).map(|_| ())
    }

    /// Draws a list of objects with clipping.
    ///
    /// This function sends a message to the graphics server to draw the specified list of objects
    /// within the specified `ClipRect` clip area.
    ///
    /// <details>
    ///     <summary>Example Image - two objects, a line and a circle, each with their own clipping areas
    /// represented by light rectangles.</summary>
    ///
    /// ![Example Image](https://github.com/betrusted-io/xous-core/blob/main/docs/images/graphics-server/object_list_clipped.png?raw=true)
    ///
    /// </details>
    ///
    /// # Example
    /// ```
    /// use graphics_server::{ClipObjectList, Gfx, Point, Rectangle};
    /// let gfx = Gfx::new(&xous_names::XousNames::new().unwrap()).unwrap();
    /// let mut object_list = ClipObjectList::default();
    ///
    /// // Create and add the first object (a line)
    /// let line = Line::new(Point::new(0, 0), Point::new(100, 100));
    /// let clip = Rectangle::new(Point::new(10, 10), Point::new(90, 90));
    /// object_list.push(ClipObjectType::Line(line), clip);
    ///
    /// // Create and add the second object (a circle)
    /// let circle = Circle::new(Point::new(150, 150), 25);
    /// let clip2 = Rectangle::new(Point::new(140, 100), Point::new(190, 190));
    /// object_list.push(ClipObjectType::Circ(circle), clip2);
    ///
    /// gfx.draw_object_list_clipped(object_list).unwrap();
    /// gfx.flush().unwrap();
    /// ```
    pub fn draw_object_list_clipped(&self, list: ClipObjectList) -> Result<(), xous::Error> {
        let buf = Buffer::into_buf(list).or(Err(xous::Error::InternalError))?;
        buf.lend(self.conn, GfxOpcode::DrawClipObjectList.to_u32().unwrap()).map(|_| ())
    }

    /// Sets the developer boot mode.
    ///
    /// This function sends a message to the graphics server to enable or disable the developer boot mode.
    /// The purpose of this call is to let users be aware of who signed their kernel. Kernels signed with the
    /// developer signature can boot, but this API call ensures that a small dashed line appears through the
    /// status bar, so there is an obvious indicator that the kernel has yet to be self-signed. Self-signed
    /// kernels do not have this signature.
    ///
    /// NOTE: This call is a one-way operation. Once the developer boot mode is enabled, it cannot be
    /// disabled.
    ///
    /// # Example
    /// ```
    /// use graphics_server::Gfx;
    /// let gfx = Gfx::new(&xous_names::XousNames::new().unwrap()).unwrap();
    /// gfx.set_devboot(true); // Enable developer boot mode
    /// ```
    pub fn set_devboot(&self, enable: bool) -> Result<(), xous::Error> {
        let ena = if enable { 1 } else { 0 };
        send_message(self.conn, Message::new_scalar(GfxOpcode::Devboot.to_usize().unwrap(), ena, 0, 0, 0))
            .map(|_| ())
    }

    /// Reads the font map in bulk from the graphics server.
    ///
    /// Instead of implementing the read in the library, we hand the raw GfxOpcode to the caller.
    /// This allows the caller to re-use the bulk read data structure across multiple reads
    /// instead of it being re-allocated and re-initialized every single call. This is used by the security
    /// system to inspect the font maps for integrity.
    ///
    /// # Example
    /// ```
    /// use graphics_server::{ArchivedBulkRead, Gfx};
    /// let gfx = Gfx::new(&xous_names::XousNames::new().unwrap()).unwrap();
    /// let font_map = gfx.bulk_read_fontmap_op();
    /// // Process the font map as needed
    /// ```
    pub fn bulk_read_fontmap_op(&self) -> u32 { GfxOpcode::BulkReadFonts.to_u32().unwrap() }

    /// Resets the bulk read pointer.
    ///
    /// The bulk read operation auto-increments a pointer on the graphics server, so this message is necessary
    /// to reset the pointer to 0.
    ///
    /// # Example
    /// ```
    /// use graphics_server::Gfx;
    /// let gfx = Gfx::new(&xous_names::XousNames::new().unwrap()).unwrap();
    /// gfx.bulk_read_restart();
    /// ```
    pub fn bulk_read_restart(&self) {
        send_message(
            self.conn,
            Message::new_blocking_scalar(GfxOpcode::RestartBulkRead.to_usize().unwrap(), 0, 0, 0, 0),
        )
        .expect("couldn't reset bulk read");
    }

    /// Runs a self-test pattern.
    ///
    /// This function sends a message to the graphics server to display a test pattern for a specified
    /// duration. The test pattern is typically used to verify the functionality of the graphics hardware.
    ///
    /// <details>
    ///     <summary>Example Image - selftest in progress.</summary>
    ///
    /// ![Example Image](https://github.com/betrusted-io/xous-core/blob/main/docs/images/graphics-server/selftest.png?raw=true)
    ///
    /// </details>
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
            Message::new_blocking_scalar(GfxOpcode::TestPattern.to_usize().unwrap(), duration_ms, 0, 0, 0),
        )
        .expect("couldn't self test");
    }

    /// Stashes the current graphics state.
    ///
    /// This function sends a message to the graphics server to stash (save) the current graphics state.
    /// The stashed state can be restored later using the `pop` function. This is useful for temporarily
    /// saving the current state before making changes, and then restoring it later.
    ///
    /// If `blocking` is `true`, the function will wait until the graphics server confirms that the state
    /// has been stashed, otherwise it will return immediately without waiting for confirmation.
    ///
    /// NOTE: the top 34 lines will not be stashed, as this is the area where the status bar is drawn.
    ///
    /// # Example
    /// ```
    /// use graphics_server::Gfx;
    /// let gfx = Gfx::new(&xous_names::XousNames::new().unwrap()).unwrap();
    /// let line = Line::new(Point::new(0, 0), Point::new(100, 100));
    /// gfx.draw_line(line).unwrap(); // Draw a line before stashing
    /// gfx.stash(false); // Stash the current state in a non-blocking manner
    /// ```
    pub fn stash(&self, blocking: bool) {
        if blocking {
            send_message(
                self.conn,
                Message::new_blocking_scalar(GfxOpcode::Stash.to_usize().unwrap(), 0, 0, 0, 0),
            )
            .expect("couldn't stash");
        } else {
            send_message(self.conn, Message::new_scalar(GfxOpcode::Stash.to_usize().unwrap(), 0, 0, 0, 0))
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
    /// use graphics_server::{DrawStyle, Gfx, Line, PixelColor, Point, Rectangle};
    /// use ticktimer_server::Ticktimer;
    /// let gfx = Gfx::new(&xous_names::XousNames::new().unwrap()).unwrap();
    /// let ticktimer = Ticktimer::new().expect("Couldn't connect to Ticktimer");
    ///
    /// // Draw a line
    /// let line = Line::new(Point::new(0, 0), Point::new(100, 100));
    /// gfx.draw_line(line).unwrap();
    /// gfx.flush().unwrap();
    ///
    /// // Wait for a moment
    /// ticktimer.sleep_ms(1000).unwrap();
    ///
    /// // Stash the current state
    /// gfx.stash(true); // Stash the current state in a blocking manner
    ///
    /// // Clear the screen
    /// let screensize = gfx.screen_size().expect("Couldn't get screen size");
    /// let whiteout = Rectangle::new_with_style(
    ///     Point::new(0, 0),
    ///     screensize,
    ///     DrawStyle::new(PixelColor::Light, PixelColor::Light, 1),
    /// );
    /// gfx.draw_rectangle(blackout).unwrap();
    /// gfx.flush().unwrap();
    ///
    /// // Wait for a moment
    /// ticktimer.sleep_ms(1000).unwrap();
    ///
    /// // Restore the previously stashed state
    /// gfx.pop(true); // Restore the previously stashed state in a blocking manner
    /// gfx.flush().unwrap();
    /// ```
    pub fn pop(&self, blocking: bool) {
        if blocking {
            send_message(
                self.conn,
                Message::new_blocking_scalar(GfxOpcode::Pop.to_usize().unwrap(), 0, 0, 0, 0),
            )
            .expect("couldn't pop");
        } else {
            send_message(self.conn, Message::new_scalar(GfxOpcode::Pop.to_usize().unwrap(), 0, 0, 0, 0))
                .expect("couldn't pop");
        }
    }

    /// This will cause the caller to block until it is granted a lock on the modal state.
    ///
    /// This is a v2-only API. Calling it on a v1 system will cause a panic.
    pub fn acquire_modal(&self) -> Result<xous::Result, xous::Error> {
        send_message(
            self.conn,
            Message::new_blocking_scalar(GfxOpcode::AcquireModal.to_usize().unwrap(), 0, 0, 0, 0),
        )
    }

    /// This is a v2-only API. Calling it on a v1 system will cause a panic.
    pub fn release_modal(&self) -> Result<xous::Result, xous::Error> {
        send_message(self.conn, Message::new_scalar(GfxOpcode::ReleaseModal.to_usize().unwrap(), 0, 0, 0, 0))
    }

    /// V2-only API
    pub fn draw_object_list(&self, mut list: ObjectList) -> Result<(), xous::Error> {
        list.list.shrink_to_fit();
        let size = list.list.capacity() * size_of::<ClipObjectType>() + size_of::<Vec<ClipObjectType>>();
        let buf = match Buffer::into_buf(list) {
            Ok(b) => b,
            Err(e) => {
                log::info!("Size {} won't fit", size);
                log::error!("err: {:?}", e);
                panic!("error")
            }
        };
        buf.lend(self.conn, GfxOpcode::UnclippedObjectList.to_u32().unwrap()).map(|_| ())
    }

    /// On small screens, `top_left` is ignored and the code takes the whole screen, always
    pub fn render_qr(
        &self,
        qr_stream: &Vec<bool>,
        qr_width: usize,
        top_left: Point,
    ) -> Result<(), xous::Error> {
        let qr_render = QrRender { width: qr_width, top_left, modules: qr_stream.to_owned() };
        let mut buf = Buffer::into_buf(qr_render).unwrap();
        buf.lend_mut(self.conn, GfxOpcode::RenderQr.to_u32().unwrap())?;
        // do nothing with the response - function just blocks until the QR code is done rendering
        Ok(())
    }

    #[cfg(feature = "board-baosec")]
    pub fn acquire_qr(&self) -> Result<QrAcquisition, xous::Error> {
        let acquisition = QrAcquisition { content: None, meta: None };
        let mut buf = Buffer::into_buf(acquisition).unwrap();
        buf.lend_mut(self.conn, GfxOpcode::AcquireQr.to_u32().unwrap())?;
        let response: QrAcquisition = buf.to_original()?;
        Ok(response)
    }

    #[cfg(feature = "hosted-baosec")]
    pub fn acquire_qr(&self) -> Result<QrAcquisition, xous::Error> {
        let dummy = "otpauth://totp/ACME%20Co:john.doe@email.com?secret=HXDMVJECJJWSRB3HWIZR4IFUGFTMXBOZ&issuer=ACME%20Co&algorithm=SHA1&digits=6&period=30".to_string();
        // just return some dummy data
        Ok(QrAcquisition { content: Some(dummy), meta: None })
    }

    pub fn register_listener(&self, server_name: &str, action_opcode: usize) {
        let kr =
            KeyboardRegistration { server_name: String::from(server_name), listener_op_id: action_opcode };
        let buf = Buffer::into_buf(kr).unwrap();
        buf.lend(self.conn, GfxOpcode::FilteredKeyboardListener.to_u32().unwrap())
            .expect("couldn't register listener");
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
