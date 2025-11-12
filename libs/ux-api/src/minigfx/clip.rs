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

#[cfg_attr(feature = "derive-rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
#[derive(Debug, Clone)]
/// This API relies on an upgraded version of rkyv that was not available when the ClipObjectList
/// API was defined: we can now just use a `Vec` to do the trick.
#[cfg(feature = "std")]
pub struct ObjectList {
    pub list: Vec<ClipObjectType>,
}
#[cfg(feature = "std")]
impl ObjectList {
    pub fn new() -> Self { Self { list: Vec::new() } }

    /// The intent was for the push() to be infalliable, but in practice, draw lists could get
    /// arbitrarily large and some back-pressure is needed to keep the memory allocation within
    /// bounds that the system can handle. Thus, this method can fail returning the pushed object,
    /// at which point one should send the draw list to the graphics engine, and retry the push.
    pub fn push(&mut self, item: ClipObjectType) -> Result<(), ClipObjectType> {
        let serialized_size =
            self.list.capacity() * size_of::<ClipObjectType>() + size_of::<Vec<ClipObjectType>>();
        // log::debug!("sersize {}", serialized_size);
        // TODO: export the capacity limit of a buffer. The origin of the capacity limit is equal to
        // the size of a page of memory, plus 256 bytes for "scratch" area for rkyv to work in. I did
        // try to use the .replace() method with an allocation of a large enough buffer to hold the whole
        // Vec, but it seems to fail. I think it could be that the scratch space hard-coded into the IPC
        // library is not big enough...
        //
        // Update: in a later version of Rust, this is even worse - the margin seems to need to be
        // fairly large for serialization to work. Setting it to 1k short of 4096 now.
        if serialized_size < 3072 {
            if serialized_size > 2048 {
                // pushing can cause a re-allocation that breaks sending the vec. Allocs happen on roughly
                // power-of-two boundaries in this implementation, so as a heuristic flag a problem only after
                // we've exceeded half the page size
                if self.list.capacity() > self.list.len() {
                    // we're almost out of space, but we still have reserved capacity
                    self.list.push(item);
                    Ok(())
                } else {
                    // pushing it would cause a capacity bump to over our acceptable size
                    Err(item)
                }
            } else {
                // if the size is less than half a page, go ahead and do the push; a re-alloc won't bring
                // us over the page size
                self.list.push(item);
                Ok(())
            }
        } else {
            Err(item)
        }
    }
}
