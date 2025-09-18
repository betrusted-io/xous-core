const BALL_RADIUS: i16 = 10;
const MOMENTUM_LIMIT: i32 = 8;
const BORDER_WIDTH: i16 = 5;
use bao1x_hal_service::trng;
use graphics_server::{Circle, ClipObjectList, ClipObjectType, DrawStyle, PixelColor, Point, Rectangle};

pub struct Ball {
    gfx: graphics_server::Gfx,
    screensize: Point,
    ball: Circle,
    momentum: Point,
    clip: Rectangle,
    trng: trng::Trng,
}
impl Ball {
    pub fn new(xns: &xous_names::XousNames) -> Ball {
        let gfx = graphics_server::Gfx::new(xns).unwrap();
        gfx.draw_boot_logo().unwrap();

        let screensize = gfx.screen_size().unwrap();
        let mut ball = Circle::new(Point::new(screensize.x / 2, screensize.y / 2), BALL_RADIUS);
        ball.style = DrawStyle::new(PixelColor::Dark, PixelColor::Dark, 1);
        let clip = Rectangle::new(Point::new(0, 0), screensize);
        gfx.draw_circle(ball).unwrap();
        let trng = trng::Trng::new(&xns).unwrap();
        let x = ((trng.get_u32().unwrap() / 2) as i32) % (MOMENTUM_LIMIT * 2) - MOMENTUM_LIMIT;
        let y = ((trng.get_u32().unwrap() / 2) as i32) % (MOMENTUM_LIMIT * 2) - MOMENTUM_LIMIT;
        Ball { gfx, screensize, ball, momentum: Point::new(x as i16, y as i16), clip, trng }
    }

    #[allow(dead_code)]
    pub fn draw_boot(&self) { self.gfx.draw_boot_logo().ok(); }

    pub fn update(&mut self) {
        /* // for testing fonts, etc.
        use std::fmt::Write;
        let mut tv = graphics_server::TextView::new(
            graphics_server::Gid::new([0, 0, 0, 0]),
            graphics_server::TextBounds::BoundingBox(self.clip),
        );
        tv.clip_rect = Some(self.clip);
        tv.set_dry_run(false);
        tv.set_op(graphics_server::TextOp::Render);
        tv.style = graphics_server::api::GlyphStyle::Tall;
        write!(tv.text, "hello world! 😀").ok();
        self.gfx.draw_textview(&mut tv).ok();
        */
        let mut draw_list = ClipObjectList::default();

        // clear the previous location of the ball
        self.ball.style = DrawStyle::new(PixelColor::Light, PixelColor::Light, 1);
        draw_list.push(ClipObjectType::Circ(self.ball), self.clip).unwrap();

        // update the ball position based on the momentum vector
        self.ball.translate(self.momentum);

        // check if the ball hits the wall, if so, snap its position to the wall
        let mut hit_right = false;
        let mut hit_left = false;
        let mut hit_top = false;
        let mut hit_bott = false;
        if self.ball.center.x + (BALL_RADIUS + BORDER_WIDTH) >= self.screensize.x {
            hit_right = true;
            self.ball.center.x = self.screensize.x - (BALL_RADIUS + BORDER_WIDTH);
        }
        if self.ball.center.x - (BALL_RADIUS + BORDER_WIDTH) <= 0 {
            hit_left = true;
            self.ball.center.x = BALL_RADIUS + BORDER_WIDTH;
        }
        if self.ball.center.y + (BALL_RADIUS + BORDER_WIDTH) >= self.screensize.y {
            hit_bott = true;
            self.ball.center.y = self.screensize.y - (BALL_RADIUS + BORDER_WIDTH);
        }
        if self.ball.center.y - (BALL_RADIUS + BORDER_WIDTH) <= 0 {
            hit_top = true;
            self.ball.center.y = BALL_RADIUS + BORDER_WIDTH;
        }

        if hit_right || hit_left || hit_bott || hit_top {
            let mut x = ((self.trng.get_u32().unwrap() / 2) as i32) % (MOMENTUM_LIMIT * 2) - MOMENTUM_LIMIT;
            let mut y = ((self.trng.get_u32().unwrap() / 2) as i32) % (MOMENTUM_LIMIT * 2) - MOMENTUM_LIMIT;
            if hit_right {
                x = -x.abs();
            }
            if hit_left {
                x = x.abs();
            }
            if hit_top {
                y = y.abs();
            }
            if hit_bott {
                y = -y.abs();
            }
            self.momentum = Point::new(x as i16, y as i16);
        }

        // draw the new location for the ball
        self.ball.style = DrawStyle::new(PixelColor::Dark, PixelColor::Dark, 1);
        draw_list.push(ClipObjectType::Circ(self.ball), self.clip).unwrap();

        self.gfx.draw_object_list_clipped(draw_list).ok();
        self.gfx.flush().unwrap();
    }
}
