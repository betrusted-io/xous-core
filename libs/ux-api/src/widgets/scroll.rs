use core::fmt::Write;
use std::marker::PhantomData;

use blitstr2::GlyphStyle;

use crate::minigfx::{DrawStyle, Line, ObjectList, TextBounds};
use crate::{
    minigfx::{ClipObjectType, PixelColor, Point, Rectangle, TextView},
    service::{api::Gid, gfx::Gfx},
};

pub enum DividerStyle {
    None,
    Line,
    Box,
}

pub enum Direction {
    Up,
    Down,
    Left,
    Right,
}

/// This object takes an array of strings and attempts to render them
/// as a scrollabel list. The number of columns rendered is equal
/// to the dimensionality of the items passed; i.e., if two lists of
/// strings are passed, two columns will be rendered.
pub struct ScrollableList<'a> {
    pub items: Vec<Vec<String>>,
    /// corresponds to the index into `items` that should be rendered as selected
    pub select_index: (usize, usize),
    /// offset in the list that represents the top left drawable item
    pub scroll_offset: (usize, usize),
    /// amount of white space to put around a list item. Defaults to (0,0)
    pub margin: Point,
    /// the font to be used to render the list
    style: GlyphStyle,
    /// dimensions that the list needs to fit in
    pane: Rectangle,
    /// use scrollbars?
    with_scrollbars: bool,
    /// keep track of the maximum length column
    max_rows: usize,
    /// height of the glyph used
    height_hint: usize,
    /// Minimum column width for the list
    min_col_width: usize,
    gfx: Gfx,
    _marker: PhantomData<&'a ()>,
}

impl<'a> ScrollableList<'a> {
    pub fn default() -> Self {
        let xns = xous_names::XousNames::new().unwrap();
        let gfx = Gfx::new(&xns).unwrap();
        let default_style = GlyphStyle::Regular;
        let height_hint = gfx.glyph_height_hint(default_style).unwrap();
        Self {
            items: vec![vec![]],
            select_index: (0, 0),
            scroll_offset: (0, 0),
            style: default_style,
            height_hint,
            min_col_width: 16,
            margin: Point::new(0, 0),
            with_scrollbars: true,
            max_rows: 0,
            pane: Rectangle::new(Point::new(0, 0), gfx.screen_size().unwrap()),
            gfx,
            _marker: PhantomData,
        }
    }

    pub fn pane_size(&'a mut self, pane: Rectangle) -> &'a mut Self {
        self.pane = pane;
        self
    }

    pub fn style(&'a mut self, style: GlyphStyle) -> &'a mut Self {
        self.style = style;
        self.height_hint = self.gfx.glyph_height_hint(style).unwrap();
        self
    }

    pub fn add_item(&'a mut self, column: usize, item: &str) -> &'a mut Self {
        if (column + 1) > self.items.len() {
            for _ in 0..(column + 1 - self.items.len()) {
                self.items.push(vec![]);
            }
        }
        self.items[column].push(item.to_owned());
        // recompute the maximum length row
        self.max_rows = 0;
        for col in self.items.iter() {
            self.max_rows = self.max_rows.max(col.len());
        }
        self
    }

    pub fn set_margin(&'a mut self, margin: Point) -> &'a mut Self {
        self.margin = margin;
        self
    }

    pub fn set_min_col_width(&'a mut self, width: usize) -> &'a mut Self {
        self.min_col_width = width;
        self
    }

    pub fn set_with_scrollbars(&'a mut self, with_bars: bool) -> &'a mut Self {
        self.with_scrollbars = with_bars;
        self
    }

    pub fn set_selected(&mut self, row: usize, col: usize) { self.select_index = (row, col); }

    pub fn set_scroll_offset(&mut self, row: usize, col: usize) { self.scroll_offset = (row, col); }

    pub fn move_selection(&mut self, dir: Direction) {
        self.select_index = match dir {
            Direction::Up => (self.select_index.0, self.select_index.1.saturating_sub(1)),
            Direction::Down => (
                self.select_index.0,
                self.select_index.1.saturating_add(1).min(self.items[self.select_index.0].len() - 1),
            ),
            // column term is complicated because the new column maybe shorter than the previous column
            Direction::Left => (
                self.select_index.0.saturating_sub(1),
                // column term is complicated because the new column maybe shorter than the previous column
                self.select_index.1.min(self.items[self.select_index.0.saturating_sub(1)].len() - 1),
            ),
            Direction::Right => (
                self.select_index.0.saturating_add(1).min(self.items.len() - 1),
                // column term is complicated because the new column maybe shorter than the previous column
                self.select_index
                    .1
                    .min(self.items[self.select_index.0.saturating_add(1).min(self.items.len() - 1)].len()),
            ),
        };
        // this will automatically adjust the scroll to keep the selection in view
        self.ensure_selection_visible();
    }

    pub fn move_scroll_offset(&mut self, dir: Direction) {
        self.scroll_offset = match dir {
            Direction::Up => (self.scroll_offset.0, self.scroll_offset.1.saturating_sub(1)),
            Direction::Down => (
                self.scroll_offset.0,
                self.scroll_offset.1.saturating_add(1).min(self.items[self.scroll_offset.0].len() - 1),
            ),
            // column term is complicated because the new column maybe shorter than the previous column
            Direction::Left => (
                self.scroll_offset.0.saturating_sub(1),
                // column term is complicated because the new column maybe shorter than the previous column
                self.scroll_offset.1.min(self.items[self.scroll_offset.0.saturating_sub(1)].len() - 1),
            ),
            Direction::Right => (
                self.scroll_offset.0.saturating_add(1).min(self.items.len() - 1),
                // column term is complicated because the new column maybe shorter than the previous column
                self.scroll_offset
                    .1
                    .min(self.items[self.scroll_offset.0.saturating_add(1).min(self.items.len() - 1)].len()),
            ),
        }
    }

    /// Returns the string of the selected item
    pub fn get_selected(&self) -> &str { &self.items[self.select_index.0][self.select_index.1] }

    /// Returns only the index into the list of items that is selected
    pub fn get_selected_index(&self) -> (usize, usize) { self.select_index }

    /// When called, side-effets the scroll-offset to ensure that the selection index is entirely within the
    /// pane.
    pub fn ensure_selection_visible(&mut self) {
        let width = self.pane.width() as usize;
        // let height = self.pane.height();
        let col_width = (width / self.items.len()).max(self.min_col_width);

        let select_tl = Point::new(
            col_width as isize * (self.select_index.0 as isize - self.scroll_offset.0 as isize),
            self.height_hint as isize * (self.select_index.1 as isize - self.scroll_offset.1 as isize),
        );
        let select_br = select_tl + Point::new(col_width as _, self.height_hint as _);
        if !(self.pane.intersects_point(select_tl) && self.pane.intersects_point(select_br)) {
            // easy cases: snap to top and left
            if select_tl.x < self.pane.tl().x {
                self.scroll_offset.0 = self.select_index.0;
            }
            if select_tl.y < self.pane.tl().y {
                self.scroll_offset.1 = self.select_index.1;
            }
            // hard cases: figure out where the bottom is and align the top to that
            if select_br.y > self.pane.br().y {
                // how many selections in the height?
                let mut y_selections = self.pane.height() as usize / self.height_hint;
                // if it doesn't divide evenly, subtract 1 because we don't want a partial selection
                if self.pane.height() as usize % self.height_hint != 0 {
                    y_selections -= 1;
                }
                self.scroll_offset.1 = self.select_index.1.saturating_sub(y_selections);
            }
            if select_br.x > self.pane.br().y {
                let mut x_selections = self.pane.width() as usize / col_width;
                if self.pane.width() as usize % col_width != 0 {
                    x_selections -= 1;
                }
                self.scroll_offset.0 = self.select_index.0.saturating_sub(x_selections);
            }
        }
    }

    /// Draws the scrollable list based on the current state params
    pub fn draw(&self) {
        let width = self.pane.width() as usize;
        let col_width = (width / self.items.len()).max(self.min_col_width);

        let textbox =
            Rectangle::new(Point::new(0, 0), Point::new(col_width as isize, self.height_hint as isize));
        let mut tv = TextView::new(Gid::dummy(), TextBounds::BoundingBox(textbox));
        tv.margin = self.margin;
        tv.invert = true;
        tv.draw_border = false;
        tv.insertion = None;
        tv.ellipsis = true;
        self.gfx.clear().unwrap();

        let mut items_shown = vec![];
        for (col_index, column) in self.items.iter().skip(self.scroll_offset.0).enumerate() {
            let mut rows_shown = 0;
            for (row_index, item) in column.iter().skip(self.scroll_offset.1).enumerate() {
                tv.clear_str();
                tv.write_str(&item).unwrap();
                // compute selection
                if (self.select_index.0 >= self.scroll_offset.0)
                    && (self.select_index.1 >= self.scroll_offset.1)
                {
                    if (col_index + self.scroll_offset.0, row_index + self.scroll_offset.1)
                        == (self.select_index.0, self.select_index.1)
                    {
                        tv.invert = false;
                    } else {
                        tv.invert = true;
                    }
                } else {
                    tv.invert = true;
                }
                // compute bounding box
                let tl =
                    Point::new((col_width * col_index) as isize, (self.height_hint * row_index) as isize);
                let mut textbox = Rectangle::new(
                    self.pane.tl() + tl,
                    self.pane.tl() + tl + Point::new(col_width as _, self.height_hint as _),
                );
                textbox.style.stroke_width = 0;
                // don't draw if off screen
                if textbox.tl().y > self.pane.br().y {
                    break;
                }
                tv.bounds_hint = TextBounds::BoundingBox(textbox);

                self.gfx.draw_textview(&mut tv).unwrap();
                rows_shown += 1;
            }
            if textbox.br().x > self.pane.br().x {
                break;
            }
            items_shown.push(rows_shown);
        }

        if self.with_scrollbars {
            let mut ol = ObjectList::new();
            // more columns exist than displayed, draw horiz scrollbar
            if items_shown.len() < self.items.len() {
                let length = ((items_shown.len() as f32 / self.items.len() as f32) * self.pane.width() as f32)
                    as isize;
                let offset = ((self.scroll_offset.0 as f32 / self.items.len() as f32)
                    * self.pane.width() as f32) as isize;
                let mut h_rect = Rectangle::new(Point::new(0, self.pane.br().y - 3), self.pane.br());
                h_rect.style = DrawStyle::new(PixelColor::Light, PixelColor::Light, 1);
                ol.push(ClipObjectType::Rect(h_rect)).unwrap();
                let h_inner = Line::new_with_style(
                    Point::new(offset, self.pane.br().y - 2),
                    Point::new(offset + length, self.pane.br().y - 2),
                    DrawStyle::new(PixelColor::Dark, PixelColor::Dark, 1),
                );
                ol.push(ClipObjectType::Line(h_inner)).unwrap();
            }
            // more rows exist than displayed, draw vert scrollbar
            let rows_shown = *items_shown.iter().max().unwrap_or(&0);
            if rows_shown < self.max_rows {
                let length =
                    ((rows_shown as f32 / self.max_rows as f32) * self.pane.height() as f32) as isize;
                let offset = ((self.scroll_offset.1 as f32 / self.max_rows as f32)
                    * self.pane.height() as f32) as isize;
                let mut v_rect =
                    Rectangle::new(Point::new(self.pane.br().x - 3, self.pane.tl().y), self.pane.br());
                v_rect.style = DrawStyle::new(PixelColor::Light, PixelColor::Light, 1);
                ol.push(ClipObjectType::Rect(v_rect)).unwrap();
                let v_inner = Line::new_with_style(
                    Point::new(self.pane.br().x - 2, offset),
                    Point::new(self.pane.br().x - 2, offset + length),
                    DrawStyle::new(PixelColor::Dark, PixelColor::Dark, 1),
                );
                ol.push(ClipObjectType::Line(v_inner)).unwrap();
            }
            if ol.list.len() > 0 {
                self.gfx.draw_object_list(ol).unwrap();
            }
        }
        self.gfx.flush().unwrap();
    }

    /// Update the scrollable list state based on a key action. Passes on
    /// any unknown key actions.
    pub fn key_action(&mut self, k: char) -> Option<char> {
        log::trace!("key_action: {}", k);
        match k {
            '←' => {
                self.move_selection(Direction::Left);
                None
            }
            '→' => {
                self.move_selection(Direction::Right);
                None
            }
            '↑' => {
                self.move_selection(Direction::Up);
                None
            }
            '↓' => {
                self.move_selection(Direction::Down);
                None
            }
            _ => Some(k),
        }
    }
}
