use core::num::NonZeroUsize;
use std::cmp::Ordering;
use std::ops::Range;

use crate::ux::NavDir;
use crate::{SelectedEntry, VaultMode};

pub struct ListKey {
    pub name: String,
    pub guid: String,
}
impl ListKey {
    pub fn key_from_parts(name: &str, guid: &str) -> Self {
        ListKey { name: name.to_owned().to_lowercase(), guid: guid.to_owned() }
    }

    pub fn reserved() -> Self {
        ListKey { name: String::with_capacity(256), guid: String::with_capacity(256) }
    }

    // re-uses the existing storage to avoid allocations
    pub fn reset_from_parts(&mut self, name: &str, guid: &str) {
        self.name.clear();
        self.name.push_str(&name.to_lowercase());
        self.guid.clear();
        self.guid.push_str(guid);
    }
}

impl Ord for ListKey {
    fn cmp(&self, other: &Self) -> Ordering {
        match self.name.cmp(&other.name) {
            Ordering::Equal => self.guid.cmp(&other.guid),
            Ordering::Greater => Ordering::Greater,
            Ordering::Less => Ordering::Less,
        }
    }
}
impl PartialOrd<ListItem> for ListKey {
    fn partial_cmp(&self, other: &ListItem) -> Option<Ordering> {
        Some(match self.name.cmp(&other.sortable_name) {
            Ordering::Equal => self.guid.cmp(&other.guid),
            Ordering::Greater => Ordering::Greater,
            Ordering::Less => Ordering::Less,
        })
    }
}
impl PartialOrd for ListKey {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> { Some(self.cmp(other)) }
}
impl PartialEq for ListKey {
    fn eq(&self, other: &Self) -> bool { self.name == other.name && self.guid == other.guid }
}
impl Eq for ListKey {}
impl PartialEq<ListItem> for ListKey {
    fn eq(&self, other: &ListItem) -> bool { self.name == other.sortable_name && self.guid == other.guid }
}

/// Display list for items. "name" is the key by which the list is sorted.
/// "extra" is more information about the item, which should not be part of the sort.
#[derive(Debug)]
pub struct ListItem {
    name: String,
    sortable_name: String,
    pub extra: String,
    /// used by drawing routines to optimize refresh time
    pub dirty: bool,
    /// this is the name of the key used to refer to the item
    pub guid: String,
    /// stash a copy so we can compare to the DB record and avoid re-generating the atime/count string if it
    /// hasn't changed.
    pub atime: u64,
    pub count: u64,
}
impl Ord for ListItem {
    fn cmp(&self, other: &Self) -> Ordering {
        match self.sortable_name.cmp(&other.sortable_name) {
            Ordering::Equal => self.guid.cmp(&other.guid),
            Ordering::Greater => Ordering::Greater,
            Ordering::Less => Ordering::Less,
        }
    }
}
impl PartialOrd for ListItem {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> { Some(self.cmp(other)) }
}
impl PartialOrd<ListKey> for ListItem {
    fn partial_cmp(&self, other: &ListKey) -> Option<Ordering> {
        // log::info!("self {}:{}\nother: {}:{}", self.name, self.guid, other.name, other.guid);
        Some(match self.sortable_name.cmp(&other.name) {
            Ordering::Equal => self.guid.cmp(&other.guid),
            Ordering::Greater => Ordering::Greater,
            Ordering::Less => Ordering::Less,
        })
    }
}
impl PartialEq<ListKey> for ListItem {
    fn eq(&self, other: &ListKey) -> bool { self.sortable_name == other.name && self.guid == other.guid }
}
impl PartialEq for ListItem {
    fn eq(&self, other: &Self) -> bool {
        self.sortable_name == other.sortable_name && self.guid == other.guid
    }
}
impl Eq for ListItem {}

impl ListItem {
    pub fn new(name: String, extra: String, dirty: bool, guid: String, atime: u64, count: u64) -> Self {
        let sortable_name = name.to_lowercase();
        Self { name, sortable_name, extra, dirty, guid, atime, count }
    }

    /// This is made available for edit/delete routines to generate the key without having to
    /// make a whole ListItem record (which is somewhat expensive).
    pub fn key_from_parts(name: &str, guid: &str) -> String { name.to_lowercase() + &guid.to_string() }

    pub fn key(&self) -> String { Self::key_from_parts(&self.name, &self.guid) }

    pub fn name(&self) -> &String { &self.name }

    pub fn name_clear(&mut self) {
        self.name.clear();
        self.sortable_name.clear();
    }

    pub fn name_push_str(&mut self, name: &String) {
        self.name.push_str(name);
        self.sortable_name.push_str(&name.to_lowercase());
    }
}

pub struct FilteredListView {
    // You might be thinking "why use a vec when a BTreeMap can guarantee uniqueness and sorting?"
    // See PR #389 for an explanation of why that's a terrible idea. The TL;DR is that sorting is cheap
    // compared to copying data. Or more precisely, sorting is cheap compared to figuring out where
    // on the heap to copy data.
    list: Vec<ListItem>,
    sorted: bool,
    selection_index: usize,
    items_per_screen: NonZeroUsize,
    filter_range: Option<Range<usize>>,
}
#[allow(dead_code)]
impl FilteredListView {
    pub fn new() -> Self {
        Self {
            list: Vec::new(),
            sorted: false,
            selection_index: 0,
            items_per_screen: NonZeroUsize::new(1).unwrap(),
            filter_range: None,
        }
    }

    pub fn is_db_empty(&self) -> bool { self.list.len() == 0 }

    pub fn expand(&mut self, capacity: usize) { self.list.reserve(capacity.saturating_sub(self.list.len())) }

    pub fn push(&mut self, item: ListItem) {
        self.list.push(item);
        self.sorted = false;
    }

    pub fn clear(&mut self) {
        log::debug!("filter clear");
        self.list.clear();
        self.sorted = false;
        self.selection_index = 0;
        self.filter_range = None;
    }

    pub fn set_items_per_screen(&mut self, ips: usize) {
        if ips != self.items_per_screen.get() {
            for item in self.list.iter_mut() {
                item.dirty = true;
            }
        }
        self.items_per_screen = NonZeroUsize::new(ips).unwrap_or(NonZeroUsize::new(1).unwrap());
    }

    pub fn insert_unique(&mut self, item: ListItem) -> Option<ListItem> {
        if !self.sorted {
            self.list.sort();
            self.sorted = true;
        }
        self.filter_range = None;
        match self.list.binary_search_by(|probe| probe.cmp(&item)) {
            Ok(index) => Some(std::mem::replace(&mut self.list[index], item)),
            Err(index) => {
                self.list.insert(index, item);
                None
            }
        }
    }

    pub fn remove(&mut self, item: ListKey) -> Option<ListItem> {
        if !self.sorted {
            self.list.sort();
            self.sorted = true;
        }
        self.filter_range = None;
        match self.list.binary_search_by(|probe| probe.partial_cmp(&item).unwrap()) {
            Ok(index) => Some(self.list.remove(index)),
            Err(_index) => None,
        }
    }

    pub fn get(&mut self, item: &ListKey) -> Option<&mut ListItem> {
        if !self.sorted {
            self.list.sort();
            self.sorted = true;
        }
        match self.list.binary_search_by(|probe| probe.partial_cmp(item).unwrap()) {
            Ok(index) => Some(&mut self.list[index]),
            Err(_index) => None,
        }
    }

    pub fn filter(&mut self, criteria: &String) {
        //let tt = ticktimer_server::Ticktimer::new().unwrap();
        //let mut ts = [0u64; 6];
        //ts[0] = tt.elapsed_ms();
        // always ensure the list is sorted
        if !self.sorted {
            self.list.sort();
            self.sorted = true;
        }
        //ts[1] = tt.elapsed_ms();
        // sanity checks on the request
        if criteria.len() == 0 {
            self.filter_reset();
            return;
        }
        if self.list.len() == 0 {
            log::debug!("zero-length list!");
            return;
        }
        // step 1. binary search to find if the criteria is even anywhere in the list.
        match self.list.binary_search_by(|probe| probe.sortable_name.partial_cmp(criteria).unwrap()) {
            Ok(mut index) | Err(mut index) => {
                if !self.list[index].name.to_lowercase().starts_with(criteria) {
                    self.filter_range = None
                } else {
                    // step 2. we have to go backwards in the list because if we have several matches, we are
                    // not guaranteed to be at the first match. Find that first match with a linear search.
                    //ts[2] = tt.elapsed_ms();
                    while index.saturating_sub(1) > 0 {
                        if self.list[index - 1].name.to_lowercase().starts_with(criteria) {
                            index -= 1;
                        } else {
                            break;
                        }
                    }
                    // the index now starts at the range of matches. Find the end of matches with another
                    // linear search.
                    let mut end_index = index + 1;
                    //ts[3] = tt.elapsed_ms();
                    while end_index < self.list.len() {
                        if self.list[end_index].name.to_lowercase().starts_with(criteria) {
                            end_index += 1;
                        } else {
                            break;
                        }
                    }
                    self.filter_range = Some(index..end_index)
                }
            }
        }
        //ts[4] = tt.elapsed_ms();
        if let Some(r) = &self.filter_range {
            if self.selection_index >= r.len() {
                self.selection_index = 0;
            }
        }
        self.mark_filtered_as_dirty();
        //ts[5] = tt.elapsed_ms();
        //for(index, &elapsed) in ts[1..].iter().enumerate() {
        //    log::info!("{}: {}", index + 1, elapsed.saturating_sub(ts[0]));
        //}
    }

    fn mark_filtered_as_dirty(&mut self) {
        if let Some(r) = self.filter_range.clone() {
            for i in self.list[r].iter_mut() {
                i.dirty = true;
            }
        }
    }

    pub fn filter_reset(&mut self) {
        self.filter_range = Some(0..self.list.len());
        self.mark_filtered_as_dirty();
    }

    pub fn filter_len(&self) -> usize {
        if let Some(r) = &self.filter_range {
            log::debug!("filter len {}", r.len());
            r.len()
        } else {
            log::debug!("filter is not present (0)");
            0
        }
    }

    pub fn mark_all_dirty(&mut self) {
        for item in self.list.iter_mut() {
            item.dirty = true;
        }
    }

    fn mark_filtered_selection_as_dirty(&mut self, index: usize) {
        let index = index + self.filter_start();
        if index < self.list.len() {
            self.list[index].dirty = true;
        }
    }

    fn mark_filtered_screen_as_dirty(&mut self, index: usize) {
        let index = index + self.filter_start();
        let page = index / self.items_per_screen.get();
        let listlen = self.list.len();
        for item in self.list[((page as usize) * self.items_per_screen.get()).min(listlen)
            ..((1 + page as usize) * self.items_per_screen.get()).min(listlen)]
            .iter_mut()
        {
            item.dirty = true;
        }
    }

    fn filter_start(&self) -> usize { self.filter_range.clone().unwrap_or(0..0).start }

    pub fn get_page(&self) -> usize { self.selection_index / self.items_per_screen.get() }

    pub fn selected_index(&self) -> usize { self.selection_index % self.items_per_screen.get() }

    pub fn selected_page(&mut self) -> &mut [ListItem] {
        let filterlen = self.filter_len();
        let page = self.get_page();
        let filtered_range = &mut self.list[self.filter_range.clone().unwrap_or(0..0)];
        &mut filtered_range[(page * self.items_per_screen.get()).min(filterlen)
            ..((1 + page) * self.items_per_screen.get()).min(filterlen)]
    }

    pub fn nav(&mut self, dir: NavDir) {
        log::debug!("index bef: {}, filter: {:?}", self.selection_index, self.filter_range);
        match dir {
            NavDir::Up => {
                if self.selection_index > 0 {
                    let starting_page = self.get_page();
                    self.mark_filtered_selection_as_dirty(self.selection_index);
                    self.selection_index -= 1;
                    self.mark_filtered_selection_as_dirty(self.selection_index);
                    if starting_page != self.get_page() {
                        self.mark_filtered_screen_as_dirty(self.selection_index);
                    }
                }
            }
            NavDir::Down => {
                if self.selection_index < self.filter_len() {
                    let starting_page = self.get_page();
                    self.mark_filtered_selection_as_dirty(self.selection_index);
                    self.selection_index += 1;
                    self.mark_filtered_selection_as_dirty(self.selection_index);
                    if starting_page != self.get_page() {
                        self.mark_filtered_screen_as_dirty(self.selection_index);
                    }
                }
            }
            NavDir::PageUp => {
                if self.selection_index > self.items_per_screen.get() {
                    self.mark_filtered_screen_as_dirty(self.selection_index);
                    self.selection_index -= self.items_per_screen.get();
                    self.mark_filtered_screen_as_dirty(self.selection_index);
                } else {
                    self.mark_filtered_selection_as_dirty(self.selection_index);
                    self.selection_index = 0;
                    self.mark_filtered_selection_as_dirty(self.selection_index);
                }
            }
            NavDir::PageDown => {
                if self.selection_index < self.filter_len() - self.items_per_screen.get() {
                    self.mark_filtered_screen_as_dirty(self.selection_index);
                    self.selection_index += self.items_per_screen.get();
                    self.mark_filtered_screen_as_dirty(self.selection_index);
                } else {
                    self.mark_filtered_selection_as_dirty(self.selection_index);
                    self.selection_index = self.filter_len() - 1;
                    self.mark_filtered_selection_as_dirty(self.selection_index);
                }
            }
        }
        log::debug!("index after: {}", self.selection_index);
    }

    pub fn selected_guid(&self) -> String {
        self.list[self.selection_index + self.filter_start()].guid.to_owned()
    }

    pub fn selected_extra(&self) -> String {
        self.list[self.selection_index + self.filter_start()].extra.to_owned()
    }

    pub fn selected_update_extra(&mut self, extra: String) {
        let start = self.filter_start();
        self.list[self.selection_index + start].extra = extra
    }

    pub fn selected_update_atime(&mut self, atime: u64) {
        let start = self.filter_start();
        self.list[self.selection_index + start].atime = atime
    }

    pub fn selected_entry(&self, mode: VaultMode) -> Option<SelectedEntry> {
        if let Some(r) = self.filter_range.clone() {
            log::debug!("filter range: {:?}", r);
            log::debug!("selection index: {}", self.selection_index);
            log::debug!("filter start: {}", self.filter_start());
            if r.contains(&(self.selection_index + self.filter_start())) {
                Some(SelectedEntry {
                    key_guid: String::from(&self.list[self.selection_index + self.filter_start()].guid),
                    description: String::from(&self.list[self.selection_index + self.filter_start()].name),
                    mode,
                })
            } else {
                None
            }
        } else {
            None
        }
    }
}
pub struct ItemLists {
    totp: FilteredListView,
    pw: FilteredListView,
}
#[allow(dead_code)]
impl ItemLists {
    pub fn new() -> Self { ItemLists { totp: FilteredListView::new(), pw: FilteredListView::new() } }

    pub fn is_db_empty(&self, list_type: VaultMode) -> bool { self.li(list_type).is_db_empty() }

    fn li_mut(&mut self, list_type: VaultMode) -> &mut FilteredListView {
        match list_type {
            VaultMode::Totp => &mut self.totp,
            VaultMode::Password => &mut self.pw,
        }
    }

    fn li(&self, list_type: VaultMode) -> &FilteredListView {
        match list_type {
            VaultMode::Totp => &self.totp,
            VaultMode::Password => &self.pw,
        }
    }

    pub fn push(&mut self, list_type: VaultMode, item: ListItem) { self.li_mut(list_type).push(item); }

    pub fn expand(&mut self, list_type: VaultMode, capacity: usize) {
        self.li_mut(list_type).expand(capacity);
    }

    pub fn insert_unique(&mut self, list_type: VaultMode, item: ListItem) -> Option<ListItem> {
        self.li_mut(list_type).insert_unique(item)
    }

    pub fn get(&mut self, list_type: VaultMode, key: &ListKey) -> Option<&mut ListItem> {
        self.li_mut(list_type).get(key)
    }

    pub fn remove(&mut self, list_type: VaultMode, key: ListKey) -> Option<ListItem> {
        self.li_mut(list_type).remove(key)
    }

    pub fn set_items_per_screen(&mut self, ips: isize) {
        self.totp.set_items_per_screen(ips as usize);
        self.pw.set_items_per_screen(ips as usize);
    }

    pub fn mark_all_dirty(&mut self) {
        self.totp.mark_all_dirty();
        self.pw.mark_all_dirty();
    }

    pub fn clear_filter(&mut self) {}

    pub fn clear(&mut self, list_type: VaultMode) { self.li_mut(list_type).clear(); }

    pub fn clear_all(&mut self) {
        self.pw.clear();
        self.totp.clear();
    }

    /// Sets up a filter for the selected list type, returns a default selection index.
    pub fn filter(&mut self, list_type: VaultMode, criteria: &String) {
        self.li_mut(list_type).filter(criteria);
    }

    /// Sets the filter's buffer to point to the entire contents of the specified list (no filtering)
    pub fn filter_reset(&mut self, list_type: VaultMode) { self.li_mut(list_type).filter_reset(); }

    pub fn filter_len(&self, list_type: VaultMode) -> usize { self.li(list_type).filter_len() }

    pub fn nav(&mut self, list_type: VaultMode, dir: NavDir) { self.li_mut(list_type).nav(dir); }

    pub fn selected_index(&self, list_type: VaultMode) -> usize { self.li(list_type).selected_index() }

    pub fn selected_guid(&self, list_type: VaultMode) -> String { self.li(list_type).selected_guid() }

    pub fn selected_extra(&self, list_type: VaultMode) -> String { self.li(list_type).selected_extra() }

    pub fn selected_update_extra(&mut self, list_type: VaultMode, extra: String) {
        self.li_mut(list_type).selected_update_extra(extra)
    }

    pub fn selected_update_atime(&mut self, list_type: VaultMode, atime: u64) {
        self.li_mut(list_type).selected_update_atime(atime)
    }

    pub fn selected_entry(&self, list_type: VaultMode) -> Option<SelectedEntry> {
        self.li(list_type).selected_entry(list_type)
    }

    pub fn selected_page(&mut self, list_type: VaultMode) -> &mut [ListItem] {
        self.li_mut(list_type).selected_page()
    }
}
