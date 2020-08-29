#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

mod backend;
use backend::XousDisplay;

mod api;
use api::Opcode;

use embedded_graphics::{
    fonts::{Font6x8, Text},
    pixelcolor::BinaryColor,
    prelude::*,
    primitives::{Circle, Line, Rectangle, Triangle},
    style::{PrimitiveStyle, TextStyle},
};

use core::convert::TryFrom;

fn draw_boot_logo(display: &mut XousDisplay) {
    // Create styles used by the drawing operations.
    let thin_stroke = PrimitiveStyle::with_stroke(BinaryColor::On, 1);
    let thick_stroke = PrimitiveStyle::with_stroke(BinaryColor::On, 3);
    let fill = PrimitiveStyle::with_fill(BinaryColor::On);
    let text_style = TextStyle::new(Font6x8, BinaryColor::On);

    let yoffset = 10;

    // Draw a 3px wide outline around the display.
    let bottom_right = Point::zero() + display.size() - Point::new(1, 1);
    Rectangle::new(Point::zero(), bottom_right)
        .into_styled(thick_stroke)
        .draw(display)
        .unwrap();

    // Draw a triangle.
    Triangle::new(
        Point::new(16, 16 + yoffset),
        Point::new(16 + 16, 16 + yoffset),
        Point::new(16 + 8, yoffset),
    )
    .into_styled(thin_stroke)
    .draw(display)
    .unwrap();

    // Draw a filled square
    Rectangle::new(Point::new(52, yoffset), Point::new(52 + 16, 16 + yoffset))
        .into_styled(fill)
        .draw(display)
        .unwrap();

    // Draw a circle with a 3px wide stroke.
    Circle::new(Point::new(96, yoffset + 8), 8)
        .into_styled(thick_stroke)
        .draw(display)
        .unwrap();

    // Draw centered text.
    let text = "embedded-graphics";
    let width = text.len() as i32 * 6;
    Text::new(text, Point::new(64 - width / 2, 40))
        .into_styled(text_style)
        .draw(display)
        .unwrap();
}

#[xous::xous_main]
fn xmain() -> ! {
    // Create a new monochrome simulator display.
    let mut display = XousDisplay::new();

    draw_boot_logo(&mut display);

    display.redraw();

    let mut current_style = PrimitiveStyle::with_fill(BinaryColor::Off);

    let sid = xous::create_server(b"graphics-server ").unwrap();
    println!("GFX: Server listening on address {:?}", sid);
    loop {
        let msg = xous::receive_message(sid).unwrap();
        // println!("GFX: Message: {:?}", msg);
        if let Ok(opcode) = Opcode::try_from(&msg.body) {
            println!("GFX: Opcode: {:?}", opcode);
            match opcode {
                Opcode::Flush => {
                    display.update();
                    display.redraw();
                },
                Opcode::Clear(color) => {
                    let fill_color = PrimitiveStyle::with_fill(if color.color == 0 {
                        BinaryColor::Off
                    } else {
                        BinaryColor::On
                    });

                    let bottom_right = Point::zero() + display.size();
                    Rectangle::new(Point::zero(), bottom_right)
                        .into_styled(fill_color)
                        .draw(&mut display)
                        .ok();
                }
                Opcode::Line(start, end) => {
                    // println!("GFX: Drawing line from {:?} to {:?}", start, end);
                    Line::new(start.into(), end.into())
                        .into_styled(current_style)
                        .draw(&mut display)
                        .ok();
                }
                Opcode::Rectangle(start, end) => {
                    Line::new(start.into(), end.into())
                        .into_styled(current_style)
                        .draw(&mut display)
                        .ok();
                }
                Opcode::Circle(mid, radius) => {
                    Circle::new(mid.into(), radius)
                        .into_styled(current_style)
                        .draw(&mut display)
                        .unwrap();
                }
                Opcode::Style(stroke_width, stroke_color, fill_color) => {
                    current_style.stroke_width = stroke_width;
                    current_style.stroke_color = Some(if stroke_color.color == 0 {
                        BinaryColor::Off
                    } else {
                        BinaryColor::On
                    });
                    current_style.fill_color = Some(if fill_color.color == 0 {
                        BinaryColor::Off
                    } else {
                        BinaryColor::On
                    });
                }
            }
        } else {
            println!("Couldn't convert opcode");
        }
        display.update();
    }
}
