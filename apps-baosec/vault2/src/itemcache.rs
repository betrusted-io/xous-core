use core::num::NonZeroUsize;
use std::cmp::Ordering;
use std::ops::Range;

use crate::VaultMode;

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
#[derive(Debug, Clone)]
pub struct ListItem {
    // for passwords, this is the URL of the password
    name: String,
    /// This is the name, but made all lowercase so that the sort goes strictly alphabetical
    sortable_name: String,
    // for passwords, this is the username associated with the URL
    pub extra: String,
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
    pub fn new(name: String, extra: String, guid: String, atime: u64, count: u64) -> Self {
        let sortable_name = name.to_lowercase();
        Self { name, sortable_name, extra, guid, atime, count }
    }

    /// This is made available for edit/delete routines to generate the key without having to
    /// make a whole ListItem record (which is somewhat expensive).
    pub fn key_from_parts(name: &str, guid: &str) -> String { name.to_lowercase() + &guid.to_string() }

    pub fn key(&self) -> String { Self::key_from_parts(&self.name, &self.guid) }

    pub fn name(&self) -> &str { &self.name }

    pub fn extra(&self) -> &str { &self.extra }

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
    items_per_screen: NonZeroUsize,
    filter_range: Option<Range<usize>>,
}
#[allow(dead_code)]
impl FilteredListView {
    pub fn new() -> Self {
        Self {
            list: Vec::new(),
            sorted: false,
            items_per_screen: NonZeroUsize::new(1).unwrap(),
            filter_range: None,
        }
    }

    pub fn find_by_name(&self, name: &str) -> Vec<ListItem> {
        self.list.iter().filter(|item| item.name() == name).cloned().collect()
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
        self.filter_range = None;
    }

    pub fn set_items_per_screen(&mut self, ips: usize) {
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
    }

    pub fn filter_reset(&mut self) { self.filter_range = Some(0..self.list.len()); }

    pub fn filter_len(&self) -> usize {
        if let Some(r) = &self.filter_range {
            log::debug!("filter len {}", r.len());
            r.len()
        } else {
            log::debug!("filter is not present (0)");
            0
        }
    }

    fn filter_start(&self) -> usize { self.filter_range.clone().unwrap_or(0..0).start }

    pub fn full_list(&mut self) -> &mut [ListItem] { &mut self.list[..] }
}
pub struct ItemLists {
    totp: FilteredListView,
    pw: FilteredListView,
}
#[allow(dead_code)]
impl ItemLists {
    pub fn new() -> Self { ItemLists { totp: FilteredListView::new(), pw: FilteredListView::new() } }

    pub fn is_db_empty(&self, list_type: VaultMode) -> bool { self.li(list_type).is_db_empty() }

    pub fn find_by_name(&self, mode: VaultMode, name: &str) -> Vec<ListItem> {
        let view = match mode {
            VaultMode::Password => &self.pw,
            VaultMode::Totp => &self.totp,
        };
        view.find_by_name(name)
    }

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

    pub fn full_list(&mut self, list_type: VaultMode) -> &mut [ListItem] {
        self.li_mut(list_type).full_list()
    }
}
