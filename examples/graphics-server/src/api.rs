use xous::{Message, ScalarMessage};

#[derive(Copy, Clone, Debug)]
pub struct Point {
    x: u16,
    y: u16,
}

impl Point {
    #[allow(dead_code)]
    pub fn new(x: u16, y: u16) -> Point {
        Point { x, y }
    }
}

impl Into<usize> for Point {
    fn into(self) -> usize {
        (self.x as usize) << 16 | (self.y as usize)
    }
}

impl Into<embedded_graphics::geometry::Point> for Point {
    fn into(self) -> embedded_graphics::geometry::Point {
        embedded_graphics::geometry::Point::new(self.x as _, self.y as _)
    }
}

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
pub enum Opcode {
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
}

impl<'a> core::convert::TryFrom<&'a Message> for Opcode {
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
                _ => Err("unrecognized opcode"),
            },
            _ => Err("unhandled message type"),
        }
    }
}

impl Into<Message> for Opcode {
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
        }
    }
}
