use std::marker::PhantomData;

use blitstr2::GlyphStyle;

use crate::{
    minigfx::{Point, Rectangle},
    service::gfx::Gfx,
};

pub enum DividerStyle {
    None,
    Line,
    Box,
}

pub enum ScrollableColumns {
    S1,
    S2,
    S3,
    S4,
    S5,
    S6,
    S7,
    S8,
}

/// This object takes arbitrary lists of strings and draws
/// them in a scrollable list. The list can scroll in the vertical
/// direction. If columns > 1, then the list can be split into N columns,
/// which are navigable horizontally using the left/right keys.
pub struct ScrollableList<'a> {
    pub items: Vec<String>,
    /// corresponds to the index into `items` that should be rendered as selected
    pub select_index: usize,
    pub columns: ScrollableColumns,
    /// if true, then list items are drawn filling downward into a row first before skipping to a column
    pub row_first: bool,
    /// the font to be used to render the list
    style: GlyphStyle,
    /// dimensions that the list needs to fit in
    pane: Rectangle,
    height_hint: usize,
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
            items: vec![],
            select_index: 0,
            columns: ScrollableColumns::S1,
            row_first: true,
            style: default_style,
            height_hint,
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

    pub fn columns(&'a mut self, columns: ScrollableColumns) -> &'a mut Self {
        self.columns = columns;
        self
    }

    pub fn row_first(&'a mut self) -> &'a mut Self {
        self.row_first = true;
        self
    }

    pub fn col_first(&'a mut self) -> &'a mut Self {
        self.row_first = false;
        self
    }

    pub fn add_item(&'a mut self, item: &str) -> &'a mut Self {
        self.items.push(item.to_owned());
        self
    }

    /// Returns the string of the selected item
    pub fn get_selected(&self) -> &str { &self.items[self.select_index] }

    /// Returns only the index into the list of items that is selected
    pub fn get_selected_index(&self) -> usize { self.select_index }

    /// Draws the scrollable list based on the current state params
    pub fn draw(&self) { todo!() }

    /// Update the scrollable list state based on a key action. This also causes draw() to be called, if
    /// necessary.
    pub fn key_action(&mut self, k: char) { todo!() }
}
