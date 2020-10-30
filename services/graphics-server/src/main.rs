#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

#[cfg(target_os = "none")]
#[macro_use]
mod debug;

mod backend;
use backend::XousDisplay;

mod api;
use api::Opcode;

mod op;
mod fonts;

use core::convert::TryFrom;

mod logo;

fn draw_boot_logo(display: &mut XousDisplay) {
    display.blit_screen(logo::LOGO_MAP);
}

#[xous::xous_main]
fn xmain() -> ! {
    // Create a new monochrome simulator display.
    let mut display = XousDisplay::new();

    draw_boot_logo(&mut display);

    let mut current_color = api::Color::from(0usize);

    display.redraw();

    let sid = xous::create_server(b"graphics-server ").unwrap();
    // println!("GFX: Server listening on address {:?}", sid);
    // ::debug_here::debug_here!();
    loop {
        let msg = xous::receive_message(sid).unwrap();
        // println!("GFX: Message: {:?}", msg);
        if let Ok(opcode) = Opcode::try_from(&msg.body) {
            // println!("GFX: Opcode: {:?}", opcode);
            match opcode {
                Opcode::Flush => {
                    display.update();
                    display.redraw();
                },
                Opcode::Clear(_color) => {
                    op::clear_region(display.native_buffer(), op::ClipRegion::screen());
                }
                Opcode::Line(start, end) => {
                    println!("GFX: Drawing line from {:?} to {:?}", start, end);
                    op::line(display.native_buffer(), start.x as _, start.y as _, end.x as _, end.y as _, if current_color.color == 0 { op::PixelColor::Off } else {op::PixelColor::On });
                    // todo!();
                }
                Opcode::Rectangle(start, end) => {
                    todo!();
                    // Line::new(start.into(), end.into())
                    //     .into_styled(current_style)
                    //     .draw(&mut display)
                    //     .ok();
                }
                Opcode::Circle(mid, radius) => {
                    todo!();
                    // Circle::new(mid.into(), radius)
                    //     .into_styled(current_style)
                    //     .draw(&mut display)
                    //     .unwrap();
                }
                Opcode::Style(stroke_width, stroke_color, fill_color) => {
                    current_color = stroke_color;
                    // todo!();
                    // current_style.stroke_width = stroke_width;
                    // current_style.stroke_color = Some(if stroke_color.color == 0 {
                    //     BinaryColor::Off
                    // } else {
                    //     BinaryColor::On
                    // });
                    // current_style.fill_color = Some(if fill_color.color == 0 {
                    //     BinaryColor::Off
                    // } else {
                    //     BinaryColor::On
                    // });
                }
                Opcode::ClearRegion(rect) => {
                    op::clear_region(display.native_buffer(), op::ClipRegion {
                        x0: rect.x0 as _,
                        y0: rect.y0 as _,
                        x1: rect.x1 as _,
                        y1: rect.y1 as _,
                    });
                }
                Opcode::String(s) => {
                    op::string_regular_left(display.native_buffer(), op::ClipRegion::screen(), s);
                }
            }
        } else {
            // println!("Couldn't convert opcode");
        }
        // if let Some(mem) = msg.body.memory() {
        //     xous::return_memory(msg.sender, *mem).expect("couldn't return message");
        // }
        display.update();
    }
}
