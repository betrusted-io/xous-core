use xous::{Message, ScalarMessage};

#[derive(Copy, Clone, Debug)]
pub struct Point {
    pub x: u16,
    pub y: u16,
}

impl Point {
    pub fn new(x: u16, y: u16) -> Point {
        Point { x, y }
    }
}

impl Into<usize> for Point {
    fn into(self) -> usize {
        (self.x as usize) << 16 | (self.y as usize)
    }
}

#[derive(Copy, Clone, Debug)]
pub struct Rect {
    pub x0: u16,
    pub y0: u16,
    pub x1: u16,
    pub y1: u16,
}

impl Rect {
    pub fn new(x0: u16, y0: u16, x1: u16, y1: u16) -> Self {
        Self { x0, y0, x1, y1 }
    }
}

// impl Into<embedded_graphics::geometry::Point> for Point {
//     fn into(self) -> embedded_graphics::geometry::Point {
//         embedded_graphics::geometry::Point::new(self.x as _, self.y as _)
//     }
// }

impl From<usize> for Point {
    fn from(p: usize) -> Point {
        Point {
            x: (p >> 16 & 0xffff) as _,
            y: (p & 0xffff) as _,
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub struct Color {
    pub color: u32,
}

impl From<usize> for Color {
    fn from(c: usize) -> Color {
        Color { color: c as _ }
    }
}

#[derive(Debug)]
pub enum Opcode<'a> {
    /// Flush the buffer to the screen
    Flush,

    /// Clear the buffer to the specified color
    Clear(Color),

    /// Draw a line at the specified area
    Line(Point /* start */, Point /* end */),

    /// Draw a rectangle or square at the specified coordinates
    Rectangle(Point /* upper-left */, Point /* lower-right */),

    /// Draw a circle with a specified radius
    Circle(Point, u32 /* radius */),

    /// Change the style of the current pen
    Style(
        u32,   /* stroke width */
        Color, /* stroke color */
        Color, /* fill color */
    ),

    /// Clear the specified region
    ClearRegion(Rect),

    /// Render the string at the (x,y) coordinates
    String(&'a str),
}

impl<'a> core::convert::TryFrom<&'a Message> for Opcode<'a> {
    type Error = &'static str;
    fn try_from(message: &'a Message) -> Result<Self, Self::Error> {
        match message {
            Message::Scalar(m) => match m.id {
                1 => Ok(Opcode::Flush),
                2 => Ok(Opcode::Clear(Color::from(m.arg1))),
                3 => Ok(Opcode::Line(Point::from(m.arg1), Point::from(m.arg2))),
                4 => Ok(Opcode::Rectangle(Point::from(m.arg1), Point::from(m.arg2))),
                5 => Ok(Opcode::Circle(Point::from(m.arg1), m.arg2 as _)),
                6 => Ok(Opcode::Style(
                    m.arg1 as _,
                    Color::from(m.arg2),
                    Color::from(m.arg3),
                )),
                7 => Ok(Opcode::ClearRegion(Rect::new(
                    m.arg1 as _,
                    m.arg2 as _,
                    m.arg3 as _,
                    m.arg4 as _,
                ))),
                _ => Err("unrecognized opcode"),
            },
            Message::Borrow(m) => match m.id {
                1 => {
                    let s = unsafe {
                        core::slice::from_raw_parts(
                            m.buf.as_ptr(),
                            m.valid.map(|x| x.get()).unwrap_or_else(|| m.buf.len()),
                        )
                    };
                    Ok(Opcode::String(core::str::from_utf8(s).unwrap()))
                }
                _ => Err("unrecognized opcode"),
            },
            _ => Err("unhandled message type"),
        }
    }
}

impl<'a> Into<Message> for Opcode<'a> {
    fn into(self) -> Message {
        match self {
            Opcode::Flush => Message::Scalar(ScalarMessage {
                id: 1,
                arg1: 0,
                arg2: 0,
                arg3: 0,
                arg4: 0,
            }),
            Opcode::Clear(color) => Message::Scalar(ScalarMessage {
                id: 2,
                arg1: color.color as _,
                arg2: 0,
                arg3: 0,
                arg4: 0,
            }),
            Opcode::Line(start, end) => Message::Scalar(ScalarMessage {
                id: 3,
                arg1: start.into(),
                arg2: end.into(),
                arg3: 0,
                arg4: 0,
            }),
            Opcode::Rectangle(start, end) => Message::Scalar(ScalarMessage {
                id: 4,
                arg1: start.into(),
                arg2: end.into(),
                arg3: 0,
                arg4: 0,
            }),
            Opcode::Circle(center, radius) => Message::Scalar(ScalarMessage {
                id: 5,
                arg1: center.into(),
                arg2: radius as usize,
                arg3: 0,
                arg4: 0,
            }),
            Opcode::Style(stroke_width, stroke_color, fill_color) => {
                Message::Scalar(ScalarMessage {
                    id: 6,
                    arg1: stroke_width as _,
                    arg2: stroke_color.color as _,
                    arg3: fill_color.color as _,
                    arg4: 0,
                })
            }
            Opcode::ClearRegion(rect) => Message::Scalar(ScalarMessage {
                id: 7,
                arg1: rect.x0 as _,
                arg2: rect.y0 as _,
                arg3: rect.x1 as _,
                arg4: rect.y1 as _,
            }),
            Opcode::String(string) => {
                let region = xous::carton::Carton::from_bytes(string.as_bytes());
                Message::Borrow(region.into_message(1))
            }
        }
    }
}
