use core::fmt::Write;
use std::cell::RefCell;

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

#[derive(Clone, Copy, Debug)]
pub enum TextAlignment {
    Left,
    Center,
}

/// This object takes an array of strings and attempts to render them
/// as a scrollable list. The number of columns rendered is equal
/// to the dimensionality of the items passed; i.e., if two lists of
/// strings are passed, two columns will be rendered.
#[derive(Debug)]
pub struct ScrollableList {
    items: Vec<Vec<String>>,
    /// corresponds to the index into `items` that should be rendered as selected
    select_index: (usize, usize),
    /// offset in the list that represents the top left drawable item
    scroll_offset: (usize, usize),
    /// amount of white space to put around a list item. Defaults to (0,0)
    margin: Point,
    /// the font to be used to render the list
    style: GlyphStyle,
    /// dimensions that the list needs to fit in
    pane: RefCell<Rectangle>,
    /// use scrollbars?
    with_scrollbars: bool,
    /// keep track of the maximum length column
    max_rows: usize,
    /// height of the glyph used
    height_hint: usize,
    /// Minimum column width for the list
    min_col_width: usize,
    text_alignment: TextAlignment,
    /// This is public so we can "reach around" the abstraction and fix up some
    /// abstraction barrier bodges. Ideally this would be private, but there is need
    /// for some global shared state to lock the UI for a given model which I haven't
    /// figured out how to handle any other way.
    pub gfx: Gfx,
}

impl Clone for ScrollableList {
    fn clone(&self) -> Self {
        let mut sl = ScrollableList::default()
            .pane_size(self.pane())
            .set_min_col_width(self.min_col_width)
            .style(self.style)
            .set_with_scrollbars(self.with_scrollbars)
            .set_margin(self.margin)
            .set_alignment(self.text_alignment);
        let items = self.get_all();
        for (c, cols) in items.enumerate() {
            for row in cols {
                sl.add_item(c, row);
            }
        }
        sl.set_scroll_offset(self.scroll_offset.0, self.scroll_offset.1).ok();
        sl.set_selected(self.select_index.0, self.select_index.1).ok();
        sl
    }
}

impl ScrollableList {
    pub fn default() -> Self {
        let xns = xous_names::XousNames::new().unwrap();
        let gfx = Gfx::new(&xns).unwrap();
        let default_style = GlyphStyle::Regular;
        let height_hint = gfx.glyph_height_hint(default_style).unwrap();
        let mut pane = Rectangle::new(Point::new(0, 0), gfx.screen_size().unwrap());
        pane.style = DrawStyle::new(PixelColor::Dark, PixelColor::Dark, 1);
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
            text_alignment: TextAlignment::Left,
            pane: RefCell::new(pane),
            gfx,
        }
    }

    pub fn get_alignment(&self) -> TextAlignment { self.text_alignment }

    pub fn set_alignment(mut self, alignment: TextAlignment) -> Self {
        self.text_alignment = alignment;
        self
    }

    pub fn pane_size(mut self, pane: Rectangle) -> Self {
        self.pane = RefCell::new(pane);
        self
    }

    pub fn pane(&self) -> Rectangle { self.pane.borrow().clone() }

    pub fn style(mut self, style: GlyphStyle) -> Self {
        self.style = style;
        self.height_hint = self.gfx.glyph_height_hint(style).unwrap();
        self
    }

    pub fn get_style(&self) -> GlyphStyle { self.style }

    pub fn add_item(&mut self, column: usize, item: &str) {
        // create columns, if they don't already exist
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
    }

    /// Returns `None` if the column doesn't exist
    pub fn col_length(&self, column: usize) -> Option<usize> {
        if column < self.items.len() { Some(self.items[column].len()) } else { None }
    }

    /// If the index is out of bounds, this function will simply append the item
    pub fn insert_item(&mut self, column: usize, index: usize, item: &str) {
        // create columns, if they don't already exist
        if (column + 1) > self.items.len() {
            for _ in 0..(column + 1 - self.items.len()) {
                self.items.push(vec![]);
            }
        }
        if index < self.items[column].len() {
            self.items[column].insert(index, item.to_owned());
        } else {
            self.items[column].push(item.to_owned());
        }
        // recompute the maximum length row
        self.max_rows = 0;
        for col in self.items.iter() {
            self.max_rows = self.max_rows.max(col.len());
        }
    }

    pub fn clear(&mut self) {
        self.items = vec![vec![]];
        self.select_index = (0, 0);
        self.scroll_offset = (0, 0);
        self.max_rows = 0;
    }

    pub fn len(&self) -> usize { self.max_rows }

    pub fn row_height(&self) -> usize { self.height_hint }

    /// Use this to add space around list items for aesthetic tuning.
    pub fn set_margin(mut self, margin: Point) -> Self {
        self.margin = margin;
        self
    }

    /// Tunes the desired minimum width of a column.
    pub fn set_min_col_width(mut self, width: usize) -> Self {
        self.min_col_width = width;
        self
    }

    /// When `with_bars` is `true`, scroll bars are rendered when only part
    /// of the list is visible on the screen.
    pub fn set_with_scrollbars(mut self, with_bars: bool) -> Self {
        self.with_scrollbars = with_bars;
        self
    }

    /// Set the selected element in the scrollable array
    ///
    /// Returns `Err(())` if the indices are out of range, without updating anything.
    pub fn set_selected(&mut self, col: usize, row: usize) -> Result<(), ()> {
        if col < self.items.len() {
            if row < self.items[col].len() {
                self.select_index = (row, col);
                Ok(())
            } else {
                Err(())
            }
        } else {
            Err(())
        }
    }

    pub fn delete_item(&mut self, col: usize, row: usize) -> String {
        if self.items[col].len() > row {
            let removed = self.items[col].remove(row);
            // ensure the selection index is valid
            if self.select_index.1 >= self.items[col].len() {
                self.select_index.1 = self.items.len().saturating_sub(1);
            }
            removed
        } else {
            "".to_owned()
        }
    }

    pub fn delete_selected(&mut self) -> String {
        if self.items[self.select_index.0].len() > self.select_index.1 {
            let removed = self.items[self.select_index.0].remove(self.select_index.1);
            if self.select_index.1 >= self.items[self.select_index.0].len() {
                self.select_index.1 = self.items.len().saturating_sub(1);
            }
            removed
        } else {
            "".to_owned()
        }
    }

    /// Set the scroll offset. The selected element is the start of list rendering, and
    /// it designates the top left element on the screen.
    ///
    /// Returns `Err(())` if the indices are out of range, without updating anything.
    pub fn set_scroll_offset(&mut self, col: usize, row: usize) -> Result<(), ()> {
        if col < self.items.len() {
            if row < self.items[col].len() {
                self.scroll_offset = (row, col);
                Ok(())
            } else {
                Err(())
            }
        } else {
            Err(())
        }
    }

    /// Attempts to move the selection in the given direction. If the direction is not moveable,
    /// the attempt does nothing. Also updates the scroll offset so that the selection is always in view.
    ///
    /// This is the intended primary pathway for interacting with the widget.
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

    /// Attempts to shift the entire pane of items by changing the rendering start point in the list.
    /// Does not update the selection point, so it is possible to "lose" the selection point when updating
    /// this.
    ///
    /// Will ignore any attempt to move to an invalid offset.
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

    /// Returns the string of the selected item. Panics if you attempt to call on an empty scrollable list.
    pub fn get_selected(&self) -> &str { &self.items[self.select_index.0][self.select_index.1] }

    /// Updates the selected item with a new contents. This is useful for e.g. toggling text based on
    /// a selection. This is infalliable because the selection is guaranteed to always be valid/in-bounds;
    /// but it will panic if you attempt to call it on an empty scrollable list.
    pub fn update_selected(&mut self, contents: &str) {
        self.items[self.select_index.0][self.select_index.1] = contents.to_owned();
    }

    /// Replaces every instance of a `original` string with `replacement` string in a given column
    /// Returns the number of instances replaced.
    pub fn replace_with(&mut self, column: usize, original: &str, replacement: &str) -> usize {
        if column >= self.items.len() {
            0
        } else {
            let mut count = 0;
            for item in self.items[column].iter_mut() {
                if item == original {
                    *item = replacement.to_string();
                    count += 1;
                }
            }
            count
        }
    }

    /// Returns all the items as a nested iterator.
    pub fn get_all(&self) -> impl Iterator<Item = impl Iterator<Item = &str>> {
        self.items.iter().map(|inner_vec| inner_vec.iter().map(|s| s.as_str()))
    }

    /// Returns a reference to the requested column; None if the column is out of range
    pub fn get_column(&self, column: usize) -> Option<&Vec<String>> {
        // create columns, if they don't already exist
        if column < self.items.len() { Some(&self.items[column]) } else { None }
    }

    /// Returns only the index into the list of items that is selected
    pub fn get_selected_index(&self) -> (usize, usize) { self.select_index }

    /// When called, side-effets the scroll-offset to ensure that the selection index is entirely within the
    /// pane.
    pub fn ensure_selection_visible(&mut self) {
        let width = self.pane.borrow().width() as usize;
        // let height = self.pane.height();
        let col_width = (width / self.items.len()).max(self.min_col_width);

        let select_tl = Point::new(
            col_width as isize * (self.select_index.0 as isize - self.scroll_offset.0 as isize),
            self.height_hint as isize * (self.select_index.1 as isize - self.scroll_offset.1 as isize),
        ) + self.pane.borrow().tl();
        let select_br = select_tl + Point::new(col_width as _, self.height_hint as _);
        if !(self.pane.borrow().intersects_point(select_tl) && self.pane.borrow().intersects_point(select_br))
        {
            // easy cases: snap to top and left
            if select_tl.x < self.pane.borrow().tl().x {
                self.scroll_offset.0 = self.select_index.0;
            }
            if select_tl.y < self.pane.borrow().tl().y {
                self.scroll_offset.1 = self.select_index.1;
            }
            // hard cases: figure out where the bottom is and align the top to that
            if select_br.y > self.pane.borrow().br().y {
                // how many selections in the height?
                let mut y_selections = self.pane.borrow().height() as usize / self.height_hint;
                // if it doesn't divide evenly, subtract 1 because we don't want a partial selection
                if self.pane.borrow().height() as usize % self.height_hint != 0 {
                    y_selections -= 1;
                }
                self.scroll_offset.1 = self.select_index.1.saturating_sub(y_selections);
            }
            if select_br.x > self.pane.borrow().br().y {
                let mut x_selections = self.pane.borrow().width() as usize / col_width;
                if self.pane.borrow().width() as usize % col_width != 0 {
                    x_selections -= 1;
                }
                self.scroll_offset.0 = self.select_index.0.saturating_sub(x_selections);
            }
        }
    }

    /// Draws the scrollable list based on the current state params
    pub fn draw(&self, at_height: isize) {
        // update the pane value with at_height, keeping the old bottom-right
        self.pane.borrow_mut().tl = Point::new(0, at_height);

        let width = self.pane.borrow().width() as usize;
        let col_width = (width / self.items.len()).max(self.min_col_width);

        let textbox =
            Rectangle::new(Point::new(0, 0), Point::new(col_width as isize, self.height_hint as isize));
        let mut tv = match self.text_alignment {
            TextAlignment::Left => TextView::new(Gid::dummy(), TextBounds::BoundingBox(textbox)),
            TextAlignment::Center => TextView::new(Gid::dummy(), TextBounds::CenteredTop(textbox)),
        };
        tv.margin = self.margin;
        tv.invert = true;
        tv.draw_border = false;
        tv.insertion = None;
        tv.ellipsis = true;
        self.gfx.draw_rectangle(self.pane.borrow().clone()).unwrap();

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
                    self.pane.borrow().tl() + tl,
                    self.pane.borrow().tl() + tl + Point::new(col_width as _, self.height_hint as _),
                );
                textbox.style.stroke_width = 0;
                // don't draw if off screen
                if textbox.tl().y > self.pane.borrow().br().y {
                    break;
                }
                tv.bounds_hint = match self.text_alignment {
                    TextAlignment::Left => TextBounds::BoundingBox(textbox),
                    TextAlignment::Center => TextBounds::CenteredTop(textbox),
                };

                self.gfx.draw_textview(&mut tv).unwrap();
                rows_shown += 1;
            }
            if textbox.br().x > self.pane.borrow().br().x {
                break;
            }
            items_shown.push(rows_shown);
        }

        if self.with_scrollbars {
            let mut ol = ObjectList::new();
            // more columns exist than displayed, draw horiz scrollbar
            if items_shown.len() < self.items.len() {
                let length = ((items_shown.len() as f32 / self.items.len() as f32)
                    * self.pane.borrow().width() as f32) as isize;
                let offset = ((self.scroll_offset.0 as f32 / self.items.len() as f32)
                    * self.pane.borrow().width() as f32) as isize;
                let mut h_rect =
                    Rectangle::new(Point::new(0, self.pane.borrow().br().y - 3), self.pane.borrow().br());
                h_rect.style = DrawStyle::new(PixelColor::Light, PixelColor::Light, 1);
                ol.push(ClipObjectType::Rect(h_rect)).unwrap();
                let h_inner = Line::new_with_style(
                    Point::new(offset, self.pane.borrow().br().y - 2),
                    Point::new(offset + length, self.pane.borrow().br().y - 2),
                    DrawStyle::new(PixelColor::Dark, PixelColor::Dark, 1),
                );
                ol.push(ClipObjectType::Line(h_inner)).unwrap();
            }
            // more rows exist than displayed, draw vert scrollbar
            let rows_shown = *items_shown.iter().max().unwrap_or(&0);
            if rows_shown < self.max_rows {
                let length = ((rows_shown as f32 / self.max_rows as f32) * self.pane.borrow().height() as f32)
                    as isize;
                let offset = ((self.scroll_offset.1 as f32 / self.max_rows as f32)
                    * self.pane.borrow().height() as f32) as isize
                    + self.pane.borrow().tl().y;
                let mut v_rect = Rectangle::new(
                    Point::new(self.pane.borrow().br().x - 3, self.pane.borrow().tl().y),
                    self.pane.borrow().br(),
                );
                v_rect.style = DrawStyle::new(PixelColor::Light, PixelColor::Light, 1);
                ol.push(ClipObjectType::Rect(v_rect)).unwrap();
                let v_inner = Line::new_with_style(
                    Point::new(self.pane.borrow().br().x - 2, offset),
                    Point::new(self.pane.borrow().br().x - 2, offset + length),
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
