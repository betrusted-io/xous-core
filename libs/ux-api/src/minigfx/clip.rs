use crate::minigfx::*;

#[cfg_attr(feature = "derive-rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
#[derive(Debug, Copy, Clone)]
pub enum ClipObjectType {
    Line(Line),
    Circ(Circle),
    Rect(Rectangle),
    RoundRect(RoundedRectangle),
    XorLine(Line),
    #[cfg(feature = "ditherpunk")]
    Tile(Tile),
}

#[cfg_attr(feature = "derive-rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
#[derive(Debug, Copy, Clone)]
pub struct ClipObject {
    pub clip: Rectangle,
    pub obj: ClipObjectType,
}

#[cfg_attr(feature = "derive-rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
#[derive(Debug, Copy, Clone)]
pub struct ClipObjectList {
    // ClipObject is 28 bytes, so 32 of these takes 896 bytes, which is less than a 4k page (the minimum
    // amount that gets remapped) we limit the length to 32 so we can use the Default initializer to set
    // the None's on the array, otherwise it gets a bit painful.
    pub list: [Option<ClipObject>; 32],
    free: usize,
}
impl ClipObjectList {
    pub fn default() -> ClipObjectList { ClipObjectList { list: Default::default(), free: 0 } }

    pub fn push(&mut self, item: ClipObjectType, clip: Rectangle) -> Result<(), ClipObjectType> {
        if self.free < self.list.len() {
            self.list[self.free] = Some(ClipObject { clip, obj: item });
            self.free += 1;
            Ok(())
        } else {
            Err(item)
        }
    }
}
