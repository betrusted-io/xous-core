use core::num::NonZeroUsize;
use std::collections::BTreeMap;
use std::cmp::Ordering;
use crate::{VaultMode, SelectedEntry};
use crate::ux::framework::NavDir;
use std::sync::{Arc, Mutex};

/// Display list for items. "name" is the key by which the list is sorted.
/// "extra" is more information about the item, which should not be part of the sort.
#[derive(Debug)]
pub struct ListItem {
    pub name: String,
    pub extra: String,
    pub dirty: bool,
    /// this is the name of the key used to refer to the item
    pub guid: String,
    /// stash a copy so we can compare to the DB record and avoid re-generating the atime/count string if it hasn't changed.
    pub atime: u64,
    pub count: u64,
}
impl ListItem {
    pub fn clone(&self) -> ListItem {
        ListItem {
            name: self.name.to_string(),
            extra: self.extra.to_string(),
            dirty: self.dirty,
            guid: self.guid.to_string(),
            atime: self.atime,
            count: self.count,
        }
    }
    /// This is made available for edit/delete routines to generate the key without having to
    /// make a whole ListItem record (which is somewhat expensive).
    pub fn key_from_parts(name: &str, guid: &str) -> String {
        name.to_lowercase() + &guid.to_string()
    }
    pub fn key(&self) -> String {
        Self::key_from_parts(&self.name, &self.guid)
    }
}
impl Ord for ListItem {
    fn cmp(&self, other: &Self) -> Ordering {
        match self.name.cmp(&other.name) {
            Ordering::Equal => {
                self.guid.cmp(&other.guid)
            },
            Ordering::Greater => Ordering::Greater,
            Ordering::Less => Ordering::Less,
        }
    }
}
impl PartialOrd for ListItem {

    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl PartialEq for ListItem {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name && self.guid == other.guid
    }
}
impl Eq for ListItem {}

pub struct ItemLists {
    fido: BTreeMap::<String, Arc<Mutex<ListItem>>>,
    totp: BTreeMap::<String, Arc<Mutex<ListItem>>>,
    pw: BTreeMap::<String, Arc<Mutex<ListItem>>>,
    /// create a mostly static list of references that we can use to index into the corresponding database
    /// this gets around both lifetime issues by putting the references in this scope, and also gets
    /// around slow alloc issues having to deal with copies when avoiding lifetimes.
    filtered_list: Vec::<Option<Arc<Mutex<ListItem>>>>,
    /// records the longest possible length of all three lists, so filtered_list() capacity can be adjusted accordingly
    max_len: usize,
    /// memoize the filtered length so we don't have to search it every time
    valid_len: usize,
    items_per_screen: NonZeroUsize,
    selection_index: usize,
}
impl ItemLists {
    pub fn new() -> Self {
        ItemLists {
            fido: BTreeMap::new(),
            totp: BTreeMap::new(),
            pw: BTreeMap::new(),
            filtered_list: Vec::new(),
            max_len: 0,
            valid_len: 0,
            items_per_screen: NonZeroUsize::new(1).unwrap(),
            selection_index: 0,
        }
    }
    pub fn insert(&mut self, list_type: VaultMode, key: String, item: ListItem) -> Option<ListItem> {
        let maybe_replaced = match list_type {
            VaultMode::Fido => self.fido.insert(key, Arc::new(Mutex::new(item))),
            VaultMode::Totp => self.totp.insert(key, Arc::new(Mutex::new(item))),
            VaultMode::Password => self.pw.insert(key, Arc::new(Mutex::new(item))),
        };
        let new_len = match list_type {
            VaultMode::Fido => self.fido.len(),
            VaultMode::Totp => self.totp.len(),
            VaultMode::Password => self.pw.len(),
        };
        for _ in 0..new_len.saturating_sub(self.filtered_list.len()) {
            // fills in any growth with null records, guaranteeing that
            // we can always index into the vector.
            self.filtered_list.push(None);
        }
        self.max_len = new_len;
        if let Some(replaced) = maybe_replaced {
            Some(
                Arc::<_>::try_unwrap(replaced).unwrap().into_inner().unwrap()
            )
        } else {
            None
        }
    }
    pub fn get(&self, list_type: VaultMode, key: &String) -> Option<&Arc<Mutex<ListItem>>> {
        match list_type {
            VaultMode::Fido => self.fido.get(key),
            VaultMode::Password => self.pw.get(key),
            VaultMode::Totp => self.pw.get(key),
        }
    }
    pub fn remove(&mut self, list_type: VaultMode, key: String) -> Option<ListItem> {
        let maybe_item = match list_type {
            VaultMode::Fido => self.fido.remove(&key),
            VaultMode::Totp => self.totp.remove(&key),
            VaultMode::Password => self.pw.remove(&key),
        };
        if let Some(item) = maybe_item {
            self.filtered_list.retain(|x|
                if let Some(filter_item) = x {
                    if filter_item.lock().unwrap().guid == item.lock().unwrap().guid {
                        false // don't retain
                    } else {
                        true
                    }
                } else {
                    true
                }
            );
            Some(
                Arc::<_>::try_unwrap(item).unwrap().into_inner().unwrap()
            )
        } else {
            None
        }
    }
    pub fn set_items_per_screen(&mut self, ips: i16) {
        self.items_per_screen = NonZeroUsize::new(ips as usize).unwrap_or(NonZeroUsize::new(1).unwrap());
    }
    pub fn clear_filter(&mut self) {
        self.filtered_list.iter_mut().for_each(|i| *i = None);
        self.valid_len = 0;
        self.selection_index = 0;
    }
    pub fn clear(&mut self, list_type: VaultMode) {
        self.filtered_list.iter_mut().for_each(|i| *i = None);
        self.valid_len = 0;
        match list_type {
            VaultMode::Fido => self.fido.clear(),
            VaultMode::Totp => self.totp.clear(),
            VaultMode::Password => self.pw.clear(),
        }
        // maybe do a search for what is the new max_len? for now we can just "leave it", it just means we have some excess capacity which is good: less mallocs.
    }
    pub fn clear_all(&mut self) {
        self.filtered_list.iter_mut().for_each(|i| *i = None);
        self.valid_len = 0;
        self.fido.clear();
        self.totp.clear();
        self.pw.clear();
        self.max_len = 0;
    }
    /// Sets up a filter for the selected list type, returns a default selection index.
    pub fn filter(&mut self, list_type: VaultMode, criteria: &str) {
        let tt = ticktimer_server::Ticktimer::new().unwrap();
        let mut ts = [0u64; 5];
        ts[0] = tt.elapsed_ms();
        // clear the list
        self.filtered_list.iter_mut().for_each(|i| *i = None);
        ts[1] = tt.elapsed_ms();
        self.valid_len = 0;

        let itemlist = match list_type {
            VaultMode::Fido => &mut self.fido,
            VaultMode::Totp => &mut self.totp,
            VaultMode::Password => &mut self.pw,
        };
        ts[2] = tt.elapsed_ms();
        let mut filter_index = 0;
        for item in itemlist.values_mut() {
            if item.lock().unwrap().name.to_lowercase().starts_with(criteria) {
                item.lock().unwrap().dirty = true;
                self.filtered_list[filter_index] = Some(item.clone());
                filter_index += 1;
            }
        }
        ts[3] = tt.elapsed_ms();
        self.valid_len = filter_index;
        if self.selection_index >= self.valid_len {
            if self.valid_len > 0 {
                self.selection_index = self.valid_len - 1;
            } else {
                self.selection_index = 0;
            }
        }
        ts[4] = tt.elapsed_ms();
        for(index, &elapsed) in ts[1..].iter().enumerate() {
            log::info!("{}: {}", index + 1, elapsed - ts[0]);
        }
    }
    pub fn nav(&mut self, list_type: VaultMode, dir: NavDir) {
        match dir {
            NavDir::Up => {
                if self.selection_index > 0 {
                    let starting_page = self.get_page();
                    self.mark_as_dirty(self.selection_index);
                    self.selection_index -= 1;
                    self.mark_as_dirty(self.selection_index);
                    if starting_page != self.get_page() {
                        self.mark_screen_as_dirty(self.selection_index);
                    }
                }
            }
            NavDir::Down => {
                if self.selection_index < self.valid_len {
                    let starting_page = self.get_page();
                    self.mark_as_dirty(self.selection_index);
                    self.selection_index += 1;
                    self.mark_as_dirty(self.selection_index);
                    if starting_page != self.get_page() {
                        self.mark_screen_as_dirty(self.selection_index);
                    }
                }
            }
            NavDir::PageUp => {
                if self.selection_index > self.items_per_screen.get() {
                    self.mark_screen_as_dirty(self.selection_index);
                    self.selection_index -= self.items_per_screen.get();
                    self.mark_screen_as_dirty(self.selection_index);
                } else {
                    self.mark_as_dirty(self.selection_index);
                    self.selection_index = 0;
                    self.mark_as_dirty(self.selection_index);
                }
            }
            NavDir::PageDown => {
                if self.selection_index < self.valid_len - self.items_per_screen.get() {
                    self.mark_screen_as_dirty(self.selection_index);
                    self.selection_index += self.items_per_screen.get();
                    self.mark_screen_as_dirty(self.selection_index);
                } else {
                    self.mark_as_dirty(self.selection_index);
                    self.selection_index = self.valid_len - 1;
                    self.mark_as_dirty(self.selection_index);
                }
            }
        }
    }
    fn mark_as_dirty(&mut self, index: usize) {
        if self.valid_len > 0 {
            if let Some(record) = self.filtered_list[index.min(self.valid_len - 1)].as_mut() {
                record.lock().unwrap().dirty = true;
            }
        }
    }
    fn mark_screen_as_dirty(&mut self, index: usize) {
        let page = index as i16 / self.items_per_screen.get() as i16;
        for item in self.filtered_list[
            ((page as usize) * self.items_per_screen.get()).min(self.valid_len) ..
            ((1 + page as usize) * self.items_per_screen.get()).min(self.valid_len)
        ].iter_mut() {
            if let Some(record) = item {
                record.lock().unwrap().dirty = true;
            }
        }
    }
    pub fn get_page(&self) -> usize {
        self.selection_index / self.items_per_screen.get()
    }
    pub fn selected_index(&self) -> usize {
        self.selection_index % self.items_per_screen.get()
    }
    pub fn selected_page(&mut self) -> &mut [Option<Arc<Mutex<ListItem>>>] {
        let page = self.get_page();
        &mut self.filtered_list[
            (page * self.items_per_screen.get()).min(self.valid_len) ..
            ((1 + page) * self.items_per_screen.get()).min(self.valid_len)
        ]
    }
    pub fn selected_guid(&self) -> String {
        // we unwrap() here because this function is responsible for consistency of the filtered list.
        self.filtered_list[self.selection_index].as_ref().unwrap().lock().unwrap().guid.to_owned()
    }
    pub fn selected_extra(&self) -> String {
        // we unwrap() here because this function is responsible for consistency of the filtered list.
        self.filtered_list[self.selection_index].as_ref().unwrap().lock().unwrap().extra.to_owned()
    }
    pub fn selected_update_extra(&mut self, extra: String) {
        self.filtered_list[self.selection_index].as_mut().unwrap().lock().unwrap().extra = extra;
        self.filtered_list[self.selection_index].as_mut().unwrap().lock().unwrap().dirty = true;
    }
    pub fn selected_update_atime(&mut self, atime: u64) {
        self.filtered_list[self.selection_index].as_mut().unwrap().lock().unwrap().atime = atime;
        self.filtered_list[self.selection_index].as_mut().unwrap().lock().unwrap().dirty = true;
    }
    pub fn mark_selected_as_dirty(&mut self) {
        // we unwrap() here because this function is responsible for consistency of the filtered list.
        self.filtered_list[self.selection_index].as_mut().unwrap().lock().unwrap().dirty = true;
    }
    pub fn selected_entry(&self, mode: VaultMode) -> Option<SelectedEntry> {
        if self.selection_index > self.valid_len {
            None
        } else {
            if let Some(entry) = &self.filtered_list[self.selection_index] {
                Some(
                    SelectedEntry {
                        key_name: xous_ipc::String::from_str(entry.lock().unwrap().guid.to_string()),
                        description: xous_ipc::String::from_str(entry.lock().unwrap().name.to_string()),
                        mode
                    }
                )
            } else {
                None
            }
        }
    }

}