use core::fmt::Write as TextViewWrite;
use std::cell::RefCell;
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};

use bao1x_hal_service::Adc;
use blitstr2::GlyphStyle;
use chrono::Datelike;
use qrcode::{Color, QrCode};
use ux_api::minigfx::*;
use ux_api::service::api::Gid;
use ux_api::service::gfx::Gfx;
use ux_api::widgets::ScrollableList;
use xous::CID;

use crate::action_handler::SelectedEntry;
use crate::config::AttachState;
use crate::*;

const FAST_SCROLL_DELAY_MS: u64 = 1300;
const KEYUP_DELAY_MS: u64 = 100;
/// How many elements to skip through on fast scroll
const PAGE_INCREMENT: usize = 6;

const FACTORY_QR_STRING: &'static str = "test://factory-test-data-lorem-ipsum-data-data";
const FACTORY_TIMEOUT_S: u64 = 90;
const JIG_TIMEOUT: u64 = 35;
// full string on QR code needs to be factory://factory-aae949f6969-lorem-ipsum-data
pub const FACTORY_STANDALONE_STRING: &'static str = "factory-aae949f6969-lorem-ipsum-data";

#[cfg(not(feature = "uber"))]
const LOWBATT_THRESH_MV: u32 = 2380;
#[cfg(feature = "uber")]
// loading is fairly light (80 mA << 0.2C) so 3200 should be darn near empty
const LOWBATT_THRESH_MV: u32 = 3200;
// how long to stay in low batt mode before forcing a sleep
#[cfg(not(feature = "uber"))]
const LOWBATT_TIMEOUT_S: u64 = 90;
#[cfg(feature = "uber")]
const LOWBATT_TIMEOUT_S: u64 = 180;

pub const DEFAULT_FONT: GlyphStyle = GlyphStyle::Regular;
pub const FONT_LIST: [&'static str; 6] = ["regular", "tall", "mono", "bold", "large", "small"];
pub fn name_to_style(name: &str) -> Option<GlyphStyle> {
    match name {
        "regular" => Some(GlyphStyle::Regular),
        "tall" => Some(GlyphStyle::Tall),
        "mono" => Some(GlyphStyle::Monospace),
        "cjk" => Some(GlyphStyle::Cjk),
        "bold" => Some(GlyphStyle::Bold),
        "large" => Some(GlyphStyle::Large),
        "small" => Some(GlyphStyle::Small),
        _ => None,
    }
}
fn style_to_name(style: &GlyphStyle) -> String {
    match style {
        GlyphStyle::Regular => "regular".to_string(),
        GlyphStyle::Monospace => "mono".to_string(),
        GlyphStyle::Cjk => "cjk".to_string(),
        GlyphStyle::Bold => "bold".to_string(),
        GlyphStyle::Large => "large".to_string(),
        GlyphStyle::Small => "small".to_string(),
        GlyphStyle::Tall => "tall".to_string(),
        _ => "regular".to_string(),
    }
}
const VAULT_CONFIG_DICT: &'static str = "vault.config";
const VAULT_CONFIG_KEY_FONT: &'static str = "fontstyle";

#[derive(PartialEq, Eq, Clone, Copy)]
enum DisplayOrientation {
    Normal,
    UpsideDown,
}

/// This test doesn't have a "scan" state because to enter it, you need to scan.
enum StandAloneTestState {
    JogPress { seen_press: bool },
    Up { seen_up: bool },
    Down { seen_down: bool },
    Left { seen_left: bool },
    Right { seen_right: bool },
    Flip { orientation_changed: bool },
    Finish { seen_button: bool },
    Exit,
    Error(String),
}

impl StandAloneTestState {
    fn handle_input(self, k: Option<char>, err: Option<String>) -> Self {
        if let Some(e) = err {
            Self::Error(e)
        } else {
            match self {
                Self::JogPress { seen_press } => {
                    let seen_press = seen_press || k.unwrap_or('\0') == '∴';
                    if seen_press { Self::Up { seen_up: false } } else { Self::JogPress { seen_press } }
                }

                Self::Up { seen_up } => {
                    let seen_up = seen_up || k.unwrap_or('\0') == '↑';

                    if seen_up { Self::Down { seen_down: false } } else { Self::Up { seen_up } }
                }

                Self::Down { seen_down } => {
                    let seen_down = seen_down || k.unwrap_or('\0') == '↓';

                    if seen_down { Self::Left { seen_left: false } } else { Self::Down { seen_down } }
                }

                Self::Left { seen_left } => {
                    let seen_left = seen_left || k.unwrap_or('\0') == '←';

                    if seen_left { Self::Right { seen_right: false } } else { Self::Left { seen_left } }
                }

                Self::Right { seen_right } => {
                    let seen_right = seen_right || k.unwrap_or('\0') == '→';

                    if seen_right {
                        Self::Flip { orientation_changed: false }
                    } else {
                        Self::Right { seen_right }
                    }
                }

                Self::Flip { orientation_changed } => {
                    let orientation_changed =
                        orientation_changed || k.unwrap_or('\0') == '🔽' || k.unwrap_or('\0') == '🔼';

                    if orientation_changed {
                        log::info!("Resetting tour state...");
                        let pddb = Pddb::new();
                        let mut key = pddb
                            .get(DC34_DICT, DC34_TOUR, None, true, true, Some(1), None::<fn()>)
                            .expect("couldn't get PDDB key");
                        key.write(&[0]).ok();
                        pddb.sync().ok();
                        log::info!("...done!");
                        Self::Finish { seen_button: false }
                    } else {
                        Self::Flip { orientation_changed }
                    }
                }

                Self::Finish { seen_button } => {
                    let seen_button = seen_button
                        || k.unwrap_or('\0') == '←'
                        || k.unwrap_or('\0') == '→'
                        || k.unwrap_or('\0') == '🔥';
                    if seen_button { Self::Exit } else { Self::Finish { seen_button } }
                }

                other => other,
            }
        }
    }

    fn is_terminal(&self) -> bool { matches!(self, StandAloneTestState::Exit) }
}

enum FactoryTestState {
    InitWait { start_time: Instant },
    JogPress { seen_press: bool },
    Up { seen_up: bool },
    Down { seen_down: bool },
    Left { seen_left: bool },
    Right { seen_right: bool },
    MiddleScan { seen_middle: bool, got_scan: bool },
    Finish,
    Error(String),
}

impl FactoryTestState {
    fn handle_input(
        self,
        k: Option<char>,
        qr_result: Option<String>,
        err: Option<String>,
        jig_ready: bool,
    ) -> Self {
        if let Some(e) = err {
            Self::Error(e)
        } else {
            match self {
                Self::InitWait { start_time } => {
                    // the test will start on its own after a few seconds regardless of jig input
                    if jig_ready
                        || std::time::Instant::now().duration_since(start_time).as_secs() > JIG_TIMEOUT
                    {
                        if jig_ready {
                            log::info!("start reason: received message");
                        } else {
                            log::info!("start reason: timeout");
                        }
                        log::info!("_|TT|_JIG.START,_|TE|_");
                        Self::JogPress { seen_press: false }
                    } else {
                        Self::InitWait { start_time }
                    }
                }
                Self::JogPress { seen_press } => {
                    let seen_press = seen_press || k.unwrap_or('\0') == '∴';
                    if seen_press {
                        log::info!("_|TT|_INTERACT.START,_|TE|_");
                        Self::Up { seen_up: false }
                    } else {
                        Self::JogPress { seen_press }
                    }
                }

                Self::Up { seen_up } => {
                    let seen_up = seen_up || k.unwrap_or('\0') == '↑';

                    if seen_up { Self::Down { seen_down: false } } else { Self::Up { seen_up } }
                }

                Self::Down { seen_down } => {
                    let seen_down = seen_down || k.unwrap_or('\0') == '↓';

                    if seen_down { Self::Left { seen_left: false } } else { Self::Down { seen_down } }
                }

                Self::Left { seen_left } => {
                    // note: left/right is swapped because PCB is upside-down during testing
                    let seen_left = seen_left || k.unwrap_or('\0') == '→';

                    if seen_left { Self::Right { seen_right: false } } else { Self::Left { seen_left } }
                }

                Self::Right { seen_right } => {
                    // note: left/right is swapped because PCB is upside-down during testing
                    let seen_right = seen_right || k.unwrap_or('\0') == '←';

                    if seen_right {
                        Self::MiddleScan { seen_middle: false, got_scan: false }
                    } else {
                        Self::Right { seen_right }
                    }
                }

                Self::MiddleScan { seen_middle, got_scan } => {
                    let seen_middle = seen_middle || k.unwrap_or('\0') == '🔥';
                    let got_scan =
                        got_scan || if let Some(qr) = qr_result { &qr == FACTORY_QR_STRING } else { false };

                    if seen_middle && got_scan {
                        Self::Finish
                    } else {
                        Self::MiddleScan { seen_middle, got_scan }
                    }
                }

                other => other,
            }
        }
    }
}

macro_rules! tour_advance {
    ($self:expr, $k:expr;
     auto { $($from:ident => $to:ident),* $(,)? }
     custom { $($pat:pat => $body:expr),* $(,)? }
    ) => {
        match $self {
            $(
                Self::$from { seen_press } => {
                    if seen_press || is_tour_advance_key($k) {
                        Self::$to { seen_press: false }
                    } else {
                        Self::$from { seen_press }
                    }
                }
            )*
            $(
                $pat => $body,
            )*
        }
    }
}

fn is_tour_advance_key(k: char) -> bool {
    // up/down not used to advance because it's too easy to fat-finger with menu raising
    k == '←' || k == '→' || k == '🔥' // || k == '↑' || k == '↓'
}

enum TourState {
    Welcome { seen_press: bool },
    LightGeneExplainer1 { seen_press: bool },
    LightGeneExplainer2 { seen_press: bool },
    Breeding1 { seen_press: bool },
    Breeding2 { seen_press: bool },
    Breeding3 { seen_press: bool },
    Breeding4 { seen_press: bool },
    BadgeRecap { seen_press: bool },
    TokenIntro1 { seen_press: bool },
    TokenIntro2 { seen_press: bool },
    TokenIntro3 { seen_press: bool },
    InfoScreen { seen_press: bool },
    End { seen_press: bool },
    Error(String),
}

impl TourState {
    fn handle_input(self, k: char) -> Self {
        tour_advance!(self, k;
            auto {
                LightGeneExplainer1 => LightGeneExplainer2,
                LightGeneExplainer2 => Breeding1,
                Breeding1          => Breeding2,
                Breeding2          => Breeding3,
                Breeding3          => Breeding4,
                Breeding4          => BadgeRecap,
                BadgeRecap         => TokenIntro1,
                TokenIntro1        => TokenIntro2,
                TokenIntro2        => TokenIntro3,
                TokenIntro3        => InfoScreen,
                InfoScreen         => End,
                End                => End,  // terminal — stays put
            }
            custom {
                Self::Welcome { seen_press } => {
                    // only advance if the jog dial press in is discovered: the point
                    // of this screen is to make sure users are aware of this interaction
                    // pattern.
                    let seen_press = seen_press || k == '∴';
                    // log::info!("k: {}, seen_press: {:?}", k, seen_press);
                    if seen_press {
                        Self::LightGeneExplainer1 { seen_press: false }
                    } else {
                        Self::Welcome { seen_press }
                    }
                },
                Self::Error(e) => Self::Error(e)
            }
        )
    }

    fn is_terminal(&self) -> bool { matches!(self, TourState::End { seen_press: _ } | TourState::Error(_)) }
}

enum HelpState {
    BadgeRecap { seen_press: bool },
    InfoScreen { seen_press: bool },
    End { seen_press: bool },
    Error(String),
}

impl HelpState {
    fn handle_input(self, k: char) -> Self {
        tour_advance!(self, k;
            auto {
                BadgeRecap         => InfoScreen,
                InfoScreen         => End,
                End                => End,  // terminal — stays put
            }
            custom {
                Self::Error(e) => Self::Error(e)
            }
        )
    }

    fn is_terminal(&self) -> bool { matches!(self, HelpState::End { seen_press: _ } | HelpState::Error(_)) }
}

enum TokenHelpState {
    TokenRecap { seen_press: bool },
    Extension { seen_press: bool },
    InfoScreen { seen_press: bool },
    End { seen_press: bool },
    Error(String),
}

impl TokenHelpState {
    fn handle_input(self, k: char) -> Self {
        tour_advance!(self, k;
            auto {
                TokenRecap         => Extension,
                Extension          => InfoScreen,
                InfoScreen         => End,
                End                => End,  // terminal — stays put
            }
            custom {
                Self::Error(e) => Self::Error(e)
            }
        )
    }

    fn is_terminal(&self) -> bool {
        matches!(self, TokenHelpState::End { seen_press: _ } | TokenHelpState::Error(_))
    }
}
enum TokenTourState {
    TokenTour1 { seen_press: bool },
    TokenTour2 { seen_press: bool },
    TokenTour3 { seen_press: bool },
    TokenRecap { seen_press: bool },
    BrowserExtension { seen_press: bool },
    InfoScreen { seen_press: bool },
    End { seen_press: bool },
    Error(String),
}

impl TokenTourState {
    fn handle_input(self, k: char) -> Self {
        tour_advance!(self, k;
            auto {
                TokenTour1         => TokenTour2,
                TokenTour2         => TokenTour3,
                TokenTour3         => TokenRecap,
                TokenRecap         => BrowserExtension,
                BrowserExtension   => InfoScreen,
                End                => End,  // terminal — stays put
            }
            custom {
                Self::InfoScreen { seen_press } => {
                    if seen_press || is_tour_advance_key(k) {
                        crate::config::side_effect_skip_token_tour(true);
                        Self::End { seen_press: false }
                    } else {
                        Self::InfoScreen { seen_press }
                    }
                },
                Self::Error(e) => Self::Error(e)
            }
        )
    }

    fn is_terminal(&self) -> bool {
        matches!(self, TokenTourState::End { seen_press: _ } | TokenTourState::Error(_))
    }
}

enum AboutState {
    BaochipLogo { seen_press: bool },
    Bunnie { seen_press: bool },
    Cheeso { seen_press: bool },
    InfoScreen { seen_press: bool },
    Diagnostics { seen_press: bool },
    End { seen_press: bool },
    Error(String),
}

impl AboutState {
    fn handle_input(self, k: char) -> Self {
        tour_advance!(self, k;
            auto {
                Bunnie             => BaochipLogo,
                BaochipLogo        => Cheeso,
                Cheeso             => InfoScreen,
                InfoScreen         => Diagnostics,
                Diagnostics        => End,
                End                => End,  // terminal — stays put
            }
            custom {
                Self::Error(e) => Self::Error(e)
            }
        )
    }

    fn is_terminal(&self) -> bool { matches!(self, AboutState::End { seen_press: _ } | AboutState::Error(_)) }

    fn is_diagnostics(&self) -> bool { matches!(self, AboutState::Diagnostics { seen_press: _ }) }
}

/// Centralizes tunable UI parameters for TOTP
struct TotpLayout {}
impl TotpLayout {
    pub fn totp_box() -> RoundedRectangle {
        RoundedRectangle::new(Rectangle::new(Point::new(0, 0), Point::new(127, 40)), 0)
    }

    /// Vertical margin for the font because the centering algorithm also aligns-top, and we want a little
    /// more verticale space for aesthetic reasons than the centering algorithm gives by default.
    pub fn totp_font_vmargin() -> Point { Point::new(0, 4) }

    pub fn totp_margin() -> Point { Point::new(10, 0) }

    pub fn totp_font() -> GlyphStyle { GlyphStyle::ExtraLarge }

    pub fn timer_box() -> Rectangle { Rectangle::new(Point::new(0, 40), Point::new(127, 50)) }

    pub fn list_box() -> Rectangle { Rectangle::new(Point::new(0, 50), Point::new(127, 127)) }

    pub fn list_font() -> GlyphStyle { GlyphStyle::Regular }
}

pub struct VaultUi {
    #[allow(dead_code)]
    main_cid: CID,
    actions_conn: CID,
    gfx: Gfx,
    display_list: ScrollableList,
    item_lists: Arc<Mutex<ItemLists>>,
    mode: Arc<Mutex<VaultMode>>,
    animate: Arc<AtomicBool>,
    global_config: Option<Arc<Mutex<GlobalConfig>>>,
    orientation: DisplayOrientation,
    filter: String,

    /// totp redraw state
    totp_code: Option<String>,
    last_epoch: u64,

    pddb: RefCell<Pddb>,
    item_height: isize,
    style: GlyphStyle,
    screen_size: Point,

    usb_dev: usb_bao1x::UsbHid,
    last_key_time: u64,
    start_hold_time: u64,
    tt: ticktimer_server::Ticktimer,

    // various state machines
    start_time: Option<Instant>,
    factory_test: FactoryTestState,
    tour_state: TourState,
    token_tour_state: TokenTourState,
    token_help_state: TokenHelpState,
    help_state: HelpState,
    about_state: AboutState,
    standalone_test: StandAloneTestState,

    // when Some(), override the display state with this String in QR code format
    pub qr_override: Option<QrCode>,

    // adc for reading battery level
    adc: Adc,
    batt_polled: bool,
    low_batt_since: Option<Instant>,

    pub user_bitmap: Option<[u32; 512]>,
    phase: bool,
    edge: bool,
    last_mode: VaultMode,
    pub bio_loaded: bool,
}

impl VaultUi {
    pub fn new(
        xns: &xous_names::XousNames,
        cid: xous::CID,
        item_lists: Arc<Mutex<ItemLists>>,
        mode: Arc<Mutex<VaultMode>>,
        animate: Arc<AtomicBool>,
        actions_conn: xous::CID,
    ) -> Self {
        let pddb = pddb::Pddb::new();
        let mut totp_list = ScrollableList::default();
        totp_list
            .set_margin(TotpLayout::totp_margin())
            .pane_size(TotpLayout::list_box())
            .style(TotpLayout::list_font());
        totp_list.set_autoflush(false);

        let tt = ticktimer_server::Ticktimer::new().unwrap();
        let now = tt.elapsed_ms();
        let gfx = Gfx::new(&xns).unwrap();
        let style = DEFAULT_FONT;
        let glyph_height = gfx.glyph_height_hint(style).unwrap() as isize;
        let screen_size = gfx.screen_size().unwrap();
        let height = screen_size.y;
        Self {
            main_cid: cid,
            actions_conn,
            gfx,
            screen_size,
            display_list: totp_list,
            item_lists,
            mode,
            animate,
            filter: String::new(),
            orientation: DisplayOrientation::Normal,
            totp_code: None,
            last_epoch: crate::totp::get_current_unix_time().expect("couldn't get current time") / 30,
            pddb: RefCell::new(pddb),
            item_height: height / glyph_height,
            style,
            usb_dev: usb_bao1x::UsbHid::new(),
            tt,
            last_key_time: now,
            start_hold_time: now,
            start_time: None,
            factory_test: FactoryTestState::InitWait { start_time: std::time::Instant::now() },
            tour_state: TourState::Welcome { seen_press: false },
            token_tour_state: TokenTourState::TokenTour1 { seen_press: false },
            help_state: HelpState::BadgeRecap { seen_press: false },
            about_state: AboutState::Bunnie { seen_press: false },
            standalone_test: StandAloneTestState::JogPress { seen_press: false },
            token_help_state: TokenHelpState::TokenRecap { seen_press: false },
            global_config: None,
            qr_override: None,
            adc: Adc::new(),
            batt_polled: false,
            low_batt_since: None,
            user_bitmap: None,
            phase: false,
            edge: false,
            last_mode: VaultMode::FactoryTest,
            bio_loaded: false,
        }
    }

    pub fn reset_help_state(&mut self) { self.help_state = HelpState::BadgeRecap { seen_press: false }; }

    pub fn reset_about_state(&mut self) { self.about_state = AboutState::Bunnie { seen_press: false }; }

    pub fn reset_token_help_state(&mut self) {
        self.token_help_state = TokenHelpState::TokenRecap { seen_press: false };
    }

    pub fn reset_factory_test(&mut self) {
        self.factory_test = FactoryTestState::InitWait { start_time: std::time::Instant::now() };
    }

    pub fn set_global_config(&mut self, config: Arc<Mutex<GlobalConfig>>) {
        self.global_config = Some(config);
    }

    pub(crate) fn refresh_draw_list(&mut self) {
        let mode = { (*self.mode.lock().unwrap()).clone() };

        let mut locked_lists = if let Ok(g) = self.item_lists.try_lock() {
            g
        } else {
            log::warn!("Couldn't get lock in refresh_draw_list; aborting the refresh");
            return;
        };
        let full_list = locked_lists.filtered_list(mode);
        self.display_list.clear();
        for item in full_list {
            self.display_list.add_item(0, &item.name());
        }
    }

    pub(crate) fn update_selected_totp_code(&mut self) -> Option<String> {
        if *self.mode.lock().unwrap() != VaultMode::Totp {
            return None;
        }
        if self.display_list.len() > 0 {
            let selected = self.display_list.get_selected();
            let mut locked_lists = self.item_lists.lock().unwrap();
            let full_list = locked_lists.full_list(VaultMode::Totp);
            if let Some(selected_item) = full_list.iter().find(|item| item.name() == selected) {
                match crate::totp::db_str_to_code(&selected_item.extra) {
                    Ok(s) => {
                        self.totp_code = Some(s.clone());
                        Some(s)
                    }
                    _ => {
                        self.totp_code = None;
                        None
                    }
                }
            } else {
                None
            }
        } else {
            None
        }
    }

    pub(crate) fn get_selected_item(&self) -> Option<ListItem> {
        let mode = *self.mode.lock().unwrap();
        if self.display_list.len() > 0 {
            let selected = self.display_list.get_selected();
            let mut locked_lists = self.item_lists.lock().unwrap();
            let full_list = locked_lists.full_list(mode);
            full_list.iter().find(|&item| item.name() == selected).cloned()
        } else {
            None
        }
    }

    pub(crate) fn selected_entry(&self) -> Option<SelectedEntry> {
        let mode = *self.mode.lock().unwrap();
        if let Some(li) = self.get_selected_item() {
            let name = li.name().to_owned();
            Some(SelectedEntry { key_guid: li.guid, description: name, mode })
        } else {
            None
        }
    }

    pub(crate) fn basis_change(&mut self) {
        self.item_lists.lock().unwrap().clear_all();
        self.display_list.clear();
    }

    pub(crate) fn store_glyph_style(&mut self, style: GlyphStyle) {
        self.pddb
            .borrow()
            .delete_key(VAULT_CONFIG_DICT, VAULT_CONFIG_KEY_FONT, Some(pddb::PDDB_DEFAULT_SYSTEM_BASIS))
            .ok();

        match self.pddb.borrow().get(
            VAULT_CONFIG_DICT,
            VAULT_CONFIG_KEY_FONT,
            Some(pddb::PDDB_DEFAULT_SYSTEM_BASIS),
            true,
            true,
            Some(32),
            Some(dc34_vault::basis_change),
        ) {
            Ok(mut style_key) => {
                style_key.write(style_to_name(&style).as_bytes()).ok();
            }
            _ => panic!("PDDB access erorr"),
        };
        self.pddb.borrow().sync().ok();
    }

    pub(crate) fn apply_glyph_style(&mut self) {
        let style = match self.pddb.borrow().get(
            VAULT_CONFIG_DICT,
            VAULT_CONFIG_KEY_FONT,
            Some(pddb::PDDB_DEFAULT_SYSTEM_BASIS),
            true,
            true,
            Some(32),
            Some(dc34_vault::basis_change),
        ) {
            Ok(mut style_key) => {
                let mut name_bytes = Vec::<u8>::new();
                match style_key.read_to_end(&mut name_bytes) {
                    Ok(_len) => {
                        log::debug!(
                            "name_bytes: {:?} {:?}",
                            name_bytes,
                            String::from_utf8(name_bytes.to_vec())
                        );
                        name_to_style(&String::from_utf8(name_bytes).unwrap_or("regular".to_string()))
                            .unwrap_or(GlyphStyle::Regular)
                    }
                    Err(_) => GlyphStyle::Regular,
                }
            }
            _ => {
                log::warn!("PDDB access error reading default glyph size");
                GlyphStyle::Regular
            }
        };
        self.display_list.style(style);
        let glyph_height = self.gfx.glyph_height_hint(style).unwrap();
        self.item_height = glyph_height as isize + 2; // +2 because of the border width
        self.item_lists
            .lock()
            .unwrap()
            .set_items_per_screen((self.screen_size.y - 2 * self.item_height) / self.item_height);
        self.style = style;
    }

    /// Clear the entire screen.
    pub fn clear_area(&self) { self.gfx.clear().ok(); }

    /// Redraw the text view onto the screen.
    pub fn redraw(&mut self) {
        // to reduce locking thrash, we cache a copy of the current mode at the top of redraw.
        let mode_at_entry = (*self.mode.lock().unwrap()).clone();

        // this check is can run at the top of every loop because the underlying implementation
        // caches the setting and only sends a message to the manager thread if there's a state change.
        if mode_at_entry == VaultMode::Idle || mode_at_entry == VaultMode::IdleDevMode {
            self.global_config.as_mut().unwrap().lock().unwrap().display_fading(true);
        } else {
            self.global_config.as_mut().unwrap().lock().unwrap().display_fading(false);
        }
        if mode_at_entry == VaultMode::Idle
            && self.global_config.as_ref().unwrap().lock().unwrap().is_developer()
        {
            // fixup any transitions that strayed to Idle instead of IdleDevMode when in developer mode
            *self.mode.lock().unwrap() = VaultMode::IdleDevMode;
        }
        log::debug!("redraw mode: {:?}", mode_at_entry);

        match mode_at_entry {
            VaultMode::Idle | VaultMode::ConfirmGene | VaultMode::IdleDevMode => {
                let now = self.tt.elapsed_ms();
                if let Some(bitmap) = self.user_bitmap.as_ref() {
                    let edge = (now / 3000) % 2 == 0;
                    if self.edge != edge || mode_at_entry != self.last_mode {
                        if self.phase {
                            self.gfx.bitmap_diffusion(bitmap, None, None).ok();
                        } else {
                            self.gfx.bitmap_diffusion(&bitmaps::dc_logo::BITMAP, None, None).ok();
                        }
                        self.phase = !self.phase;
                    }
                    self.edge = edge;
                } else {
                    self.gfx.bitmap(&bitmaps::dc_logo::BITMAP, None, None).ok();
                }

                // flag a badge mismatch, mostly for diagnostics at the factory & at the show
                let mut tv = TextView::new(
                    Gid::dummy(),
                    TextBounds::CenteredTop(Rectangle::new(Point::new(0, 127 - 12), Point::new(110, 128))),
                );
                tv.invert = true;
                tv.margin = Point::new(1, 1);
                tv.style = GlyphStyle::Bold;
                tv.draw_border = false;
                // have to move this out or we lock up
                let badge_type = self.global_config.as_ref().unwrap().lock().unwrap().badge_type();
                match self.global_config.as_ref().unwrap().lock().unwrap().attachment_state() {
                    AttachState::Mismatched => {
                        write!(tv, "Mismatched!").ok();
                        self.gfx.draw_textview(&mut tv).ok();
                    }
                    AttachState::FirstMate => {
                        write!(tv, "Attach: {:?}", badge_type).ok();
                        self.gfx.draw_textview(&mut tv).ok();
                    }
                    _ => {
                        // do nothing
                    }
                }

                // check battery voltage
                if !self.batt_polled
                    && (now / 1000) % 4 == 0
                    && !self.global_config.as_ref().unwrap().lock().unwrap().is_plugged_in()
                {
                    let voltage_code = self.adc.read_raw(
                        bao1x_hal::udma::AdcSource::Ext(bao1x_hal::udma::AdcExtChannel::Adc3),
                        Some(8),
                    );
                    let vbat_mv =
                        ((bao1x_hal::udma::Adc::raw_to_voltage(voltage_code) * 1000.0f32) / 0.318f32) as u32;
                    if vbat_mv < LOWBATT_THRESH_MV {
                        self.gfx.bitmap(&bitmaps::lowbatt::BITMAP, None, None).ok();
                        let mut msg = TextView::new(
                            Gid::dummy(),
                            TextBounds::CenteredTop(Rectangle::new(Point::new(0, 0), Point::new(127, 16))),
                        );
                        write!(msg, "Batt: {} mV", vbat_mv).ok();
                        msg.draw_border = false;
                        msg.clear_area = false;
                        msg.ellipsis = true;
                        msg.invert = true;
                        self.gfx.draw_textview(&mut msg).unwrap();

                        if let Some(since) = self.low_batt_since {
                            if std::time::Instant::now().duration_since(since).as_secs() > LOWBATT_TIMEOUT_S {
                                self.global_config.as_ref().unwrap().lock().unwrap().power_off();
                            }
                        } else {
                            self.low_batt_since = Some(std::time::Instant::now())
                        }
                    } else {
                        self.low_batt_since = None;
                    }
                    // only poll once per transition into this state
                    self.batt_polled = true;
                } else {
                    self.batt_polled = false;
                }

                // indicate developer mode
                if mode_at_entry == VaultMode::IdleDevMode {
                    let mut tv = TextView::new(
                        Gid::dummy(),
                        TextBounds::CenteredTop(Rectangle::new(
                            Point::new(0, 127 - 12),
                            Point::new(128, 128),
                        )),
                    );
                    tv.invert = true;
                    tv.margin = Point::new(1, 1);
                    tv.style = GlyphStyle::Bold;
                    tv.draw_border = false;
                    write!(tv, "DEV MODE").ok();
                    self.gfx.draw_textview(&mut tv).ok();
                }
                // indicate BIO hacks
                if self.bio_loaded && (now / 1000) % 2 == 0 {
                    let mut tv = TextView::new(
                        Gid::dummy(),
                        TextBounds::CenteredTop(Rectangle::new(
                            Point::new(0, 127 - 12),
                            Point::new(128, 128),
                        )),
                    );
                    tv.invert = true;
                    tv.margin = Point::new(1, 1);
                    tv.style = GlyphStyle::Bold;
                    tv.draw_border = false;
                    write!(tv, "BIO ACTIVE").ok();
                    self.gfx.draw_textview(&mut tv).ok();
                }

                // rate feedback
                let rate = self.global_config.as_ref().unwrap().lock().unwrap().get_mutation_rate();
                match rate {
                    MutationRate::Radioactive | MutationRate::Apocalyptic | MutationRate::Elevated => {
                        let mut msg = TextView::new(
                            Gid::dummy(),
                            TextBounds::CenteredTop(Rectangle::new(Point::new(0, 0), Point::new(127, 10))),
                        );
                        msg.style = GlyphStyle::Small;
                        if rate == MutationRate::Elevated {
                            write!(msg, "elevated").ok();
                        } else if rate == MutationRate::Radioactive {
                            write!(msg, "RADIOACTIVE").ok();
                        } else if rate == MutationRate::Apocalyptic {
                            write!(msg, "~APOCALYPTIC~").ok();
                        }
                        msg.draw_border = false;
                        msg.clear_area = false;
                        msg.ellipsis = true;
                        msg.invert = true;
                        self.gfx.draw_textview(&mut msg).unwrap();
                    }
                    _ => {}
                }
            }
            VaultMode::ShowKey { quantum } | VaultMode::ResponseGene { quantum } => {
                if let Some(code) = &self.qr_override {
                    if quantum & 7 == 0 {
                        self.clear_area();
                        let width = code.width();
                        let modules: Vec<bool> =
                            code.to_colors().into_iter().map(|c| c != Color::Light).collect();
                        self.gfx.render_qr(&modules, width, Point::new(0, 0)).ok();
                    }
                    if quantum & 7 == 6 {
                        let width = if matches!(mode_at_entry, VaultMode::ShowKey { .. }) { 14 } else { 17 };
                        let mut tv = TextView::new(
                            Gid::dummy(),
                            TextBounds::CenteredTop(Rectangle::new(
                                Point::new(64 - width, 127 - 12),
                                Point::new(64 + width, 130),
                            )),
                        );
                        tv.invert = true;
                        tv.margin = Point::new(3, -2);
                        tv.style = GlyphStyle::Bold;
                        tv.draw_border = false;
                        if matches!(mode_at_entry, VaultMode::ShowKey { .. }) {
                            write!(tv, "NCE").ok();
                        } else {
                            write!(tv, "DAT").ok();
                        }
                        self.gfx.draw_textview(&mut tv).ok();
                    }
                    if matches!(mode_at_entry, VaultMode::ShowKey { .. }) {
                        *self.mode.lock().unwrap() = VaultMode::ShowKey { quantum: quantum + 1 }
                    } else if matches!(mode_at_entry, VaultMode::ResponseGene { .. }) {
                        *self.mode.lock().unwrap() = VaultMode::ResponseGene { quantum: quantum + 1 }
                    }
                } else {
                    // if no code, go back to idle mode
                    *self.mode.lock().unwrap() = VaultMode::Idle;
                }
            }
            VaultMode::GeneScan => {
                // do nothing, should be handled by the qr acquisition code
            }
            VaultMode::Totp => {
                self.clear_area();
                // check if time is set
                if chrono::Local::now().year() < 2026 {
                    // time is not set, print a warning to set time instead of the regular UI
                    let mut tv = TextView::new(Gid::dummy(), TextBounds::CenteredTop(TotpLayout::list_box()));
                    tv.invert = true;
                    tv.margin = Point::new(0, 0);
                    tv.style = GlyphStyle::Bold;
                    tv.draw_border = false;
                    write!(tv, "Time is not set. Scan QR code to set time. See defcon.org/34b for details.")
                        .ok();
                    self.gfx.draw_textview(&mut tv).ok();
                    self.gfx.flush().ok();
                    return;
                }
                // decorative box around code
                let mut totp_box = TotpLayout::totp_box();
                totp_box.border.style = DrawStyle::new(PixelColor::Dark, PixelColor::Light, 1);
                self.gfx.draw_rounded_rectangle(totp_box).ok();

                // the TOTP code
                let mut tv = TextView::new(
                    Gid::dummy(),
                    TextBounds::CenteredTop(
                        TotpLayout::totp_box().border.translate_chain(TotpLayout::totp_font_vmargin()),
                    ),
                );
                tv.invert = true;
                tv.margin = Point::new(0, 0);
                tv.style = TotpLayout::totp_font();
                tv.draw_border = false;

                if self.totp_code.is_none() && self.display_list.len() > 0 {
                    // this handles initial population of the field
                    self.update_selected_totp_code();
                }

                match &self.totp_code {
                    Some(code) => {
                        write!(tv, "{}", code).ok();
                    }
                    _ => {
                        write!(tv, "******").ok();
                    }
                }
                self.gfx.draw_textview(&mut tv).expect("couldn't draw text");

                // list of codes to pick from
                if self.totp_code.is_some() {
                    self.display_list.draw(TotpLayout::timer_box().br().y);
                } else {
                    let mut tv = TextView::new(Gid::dummy(), TextBounds::CenteredTop(TotpLayout::list_box()));
                    tv.invert = true;
                    tv.margin = Point::new(0, 0);
                    tv.style = self.style;
                    tv.draw_border = false;
                    write!(tv, "Scan QR code to add TOTP items").ok();
                    self.gfx.draw_textview(&mut tv).ok();
                }

                // draw the timer element
                let mut object_list = ObjectList::new();
                let mut timer_box = TotpLayout::timer_box();
                timer_box.style = DrawStyle::new(PixelColor::Dark, PixelColor::Light, 1);
                object_list.push(ClipObjectType::Rect(timer_box)).unwrap();

                // draw the duration bar
                let current_time = std::time::SystemTime::now()
                    .duration_since(std::time::SystemTime::UNIX_EPOCH)
                    .map(|duration| duration.as_millis())
                    .expect("couldn't get time as millis");

                // manage the epoch as well
                let epoch = (current_time / (30 * 1000)) as u64;
                if self.last_epoch != epoch {
                    self.last_epoch = epoch;
                    self.update_selected_totp_code();
                }

                let mut timer_remaining = TotpLayout::timer_box();
                let delta = (current_time - (self.last_epoch as u128 * 30 * 1000)) as isize;
                let width = timer_remaining.width() as isize;
                let delta_width = (delta * width * 128) / (30 * 128 * 1000);
                timer_remaining.br = Point::new(width - delta_width, timer_remaining.br().y);
                timer_remaining.style = DrawStyle::new(PixelColor::Light, PixelColor::Light, 1);
                object_list.push(ClipObjectType::Rect(timer_remaining)).unwrap();
                self.gfx.draw_object_list(object_list).unwrap();
            }
            VaultMode::Password => {
                self.clear_area();
                let screensize = self.screen_size;
                // handle empty database case
                if self.item_lists.lock().unwrap().filter_len(VaultMode::Password) == 0 {
                    log::debug!("no items");
                    let mut box_text = TextView::new(
                        Gid::dummy(),
                        TextBounds::CenteredBot(Rectangle::new(
                            Point::new(0, 0),
                            Point::new(screensize.x, screensize.y / 2 + self.item_height),
                        )),
                    );
                    box_text.draw_border = false;
                    box_text.clear_area = true;
                    box_text.invert = true;
                    box_text.style = GlyphStyle::Bold;
                    if self.filter.len() == 0 {
                        write!(
                            box_text,
                            "Add passwords using QR codes via browser extension: see defcon.org/34b"
                        )
                        .ok();
                    } else {
                        write!(box_text, "No passwords matching filter: {}", &self.filter).ok();
                    }
                    self.gfx.draw_textview(&mut box_text).expect("couldn't post empty notification");
                    self.gfx.flush().ok();
                    return;
                }

                // ---- draw the top "detail info" about the selected password ----
                let mut insert_at = 0;
                if let Some(entry) = self.get_selected_item() {
                    log::debug!("rendering entry {:?}", entry);
                    // draw more data about the selected item
                    let mut box_text = TextView::new(
                        Gid::dummy(),
                        TextBounds::CenteredTop(Rectangle::new(
                            Point::new(0, insert_at),
                            Point::new(screensize.x, insert_at + self.item_height * 3),
                        )),
                    );
                    box_text.draw_border = false;
                    box_text.clear_area = false;
                    box_text.ellipsis = true;
                    box_text.style = self.style;
                    box_text.invert = true;
                    // line 1
                    write!(box_text, "{}/{} [{}]", &entry.name(), &entry.extra, entry.count).ok();
                    self.gfx.draw_textview(&mut box_text).unwrap();
                    insert_at += box_text.bounds_computed.unwrap().height() as isize;
                } else {
                    // draw just the empty rectangle around the top area if nothing is selected
                    self.gfx
                        .draw_rectangle(Rectangle::new_coords_with_style(
                            0,
                            0,
                            screensize.x,
                            self.item_height * 2,
                            DrawStyle {
                                fill_color: Some(PixelColor::Dark),
                                stroke_color: Some(PixelColor::Light),
                                stroke_width: 2,
                            },
                        ))
                        .ok();
                    log::error!("Couldn't retrieve password info to render top area");
                    insert_at = self.item_height * 2;
                };
                self.display_list.draw(insert_at);
            }
            VaultMode::FactoryTest => {
                self.clear_area();
                match &self.factory_test {
                    FactoryTestState::InitWait { start_time } => {
                        self.animate.store(true, Ordering::SeqCst);
                        self.gfx.bitmap(&bitmaps::dc_logo::BITMAP, None, None).ok();
                        // render the error message below the graphic
                        let mut msg = TextView::new(
                            Gid::dummy(),
                            TextBounds::CenteredTop(Rectangle::new(Point::new(0, 64), Point::new(127, 127))),
                        );
                        write!(
                            msg,
                            "Waiting for jig...\n{}s/{}s",
                            std::time::Instant::now().duration_since(*start_time).as_secs(),
                            JIG_TIMEOUT
                        )
                        .ok();
                        msg.draw_border = false;
                        msg.clear_area = false;
                        msg.ellipsis = true;
                        msg.invert = true;
                        self.gfx.draw_textview(&mut msg).unwrap();
                    }
                    FactoryTestState::JogPress { seen_press: _ } => {
                        if self.start_time.is_none() {
                            self.start_time = Some(Instant::now());
                        }
                        self.animate.store(true, Ordering::SeqCst);
                        self.gfx.bitmap(&bitmaps::factory_jogpress::BITMAP, None, None).ok();
                    }
                    FactoryTestState::Up { seen_up: _ } => {
                        self.gfx.bitmap(&bitmaps::factory_up::BITMAP, None, None).ok();
                    }
                    FactoryTestState::Down { seen_down: _ } => {
                        self.gfx.bitmap(&bitmaps::factory_down::BITMAP, None, None).ok();
                    }
                    FactoryTestState::Left { seen_left: _ } => {
                        self.gfx.bitmap(&bitmaps::factory_left::BITMAP, None, None).ok();
                    }
                    FactoryTestState::Right { seen_right: _ } => {
                        self.gfx.bitmap(&bitmaps::factory_right::BITMAP, None, None).ok();
                    }
                    FactoryTestState::MiddleScan { seen_middle: _, got_scan: _ } => {
                        self.gfx.bitmap(&bitmaps::factory_middlescan::BITMAP, None, None).ok();
                    }
                    FactoryTestState::Finish => {
                        self.gfx.bitmap(&bitmaps::factory_pass::BITMAP, None, None).ok();
                    }
                    FactoryTestState::Error(e) => {
                        self.gfx.bitmap(&bitmaps::factory_fail::BITMAP, None, None).ok();
                        // render the error message below the graphic
                        let mut msg = TextView::new(
                            Gid::dummy(),
                            TextBounds::CenteredTop(Rectangle::new(Point::new(0, 64), Point::new(127, 127))),
                        );
                        write!(msg, "{}", e).ok();
                        msg.draw_border = false;
                        msg.clear_area = false;
                        msg.ellipsis = true;
                        msg.invert = true;
                        self.gfx.draw_textview(&mut msg).unwrap();
                    }
                }
                let elapsed = Instant::now().duration_since(self.start_time.unwrap_or(Instant::now()));
                if elapsed.as_secs() > FACTORY_TIMEOUT_S {
                    self.factory_test = FactoryTestState::Error("Timeout!".to_string());
                }
                let mut timer = TextView::new(
                    Gid::dummy(),
                    TextBounds::CenteredBot(Rectangle::new(Point::new(32, 115), Point::new(96, 128))),
                );
                timer.style = GlyphStyle::Small;
                write!(timer, "{:.1}s", elapsed.as_secs_f32()).ok();
                timer.draw_border = false;
                timer.clear_area = false;
                timer.ellipsis = false;
                timer.invert = true;
                self.gfx.draw_textview(&mut timer).unwrap();
            }
            VaultMode::StandAloneTest => {
                self.clear_area();
                if self.orientation != DisplayOrientation::Normal {
                    self.gfx.bitmap(&bitmaps::badge_flip::BITMAP, None, None).ok();
                } else {
                    match &self.standalone_test {
                        StandAloneTestState::JogPress { seen_press: _ } => {
                            if self.start_time.is_none() {
                                self.start_time = Some(Instant::now());
                            }
                            self.animate.store(true, Ordering::SeqCst);
                            self.gfx.bitmap(&bitmaps::factory_jogpress::BITMAP, None, None).ok();
                        }
                        StandAloneTestState::Up { seen_up: _ } => {
                            self.gfx.bitmap(&bitmaps::factory_up::BITMAP, None, None).ok();
                        }
                        StandAloneTestState::Down { seen_down: _ } => {
                            self.gfx.bitmap(&bitmaps::factory_down::BITMAP, None, None).ok();
                        }
                        StandAloneTestState::Left { seen_left: _ } => {
                            self.gfx.bitmap(&bitmaps::factory_left::BITMAP, None, None).ok();
                        }
                        StandAloneTestState::Right { seen_right: _ } => {
                            self.gfx.bitmap(&bitmaps::factory_right::BITMAP, None, None).ok();
                        }
                        StandAloneTestState::Flip { orientation_changed: _ } => {
                            self.gfx.bitmap(&bitmaps::factory_flip::BITMAP, None, None).ok();
                        }
                        StandAloneTestState::Finish { seen_button: _ } => {
                            self.gfx.bitmap(&bitmaps::factory_pass::BITMAP, None, None).ok();
                        }
                        StandAloneTestState::Exit => {
                            self.gfx.bitmap(&bitmaps::factory_pass::BITMAP, None, None).ok();
                        }
                        StandAloneTestState::Error(e) => {
                            self.gfx.bitmap(&bitmaps::factory_fail::BITMAP, None, None).ok();
                            // render the error message below the graphic
                            let mut msg = TextView::new(
                                Gid::dummy(),
                                TextBounds::CenteredTop(Rectangle::new(
                                    Point::new(0, 64),
                                    Point::new(127, 127),
                                )),
                            );
                            write!(msg, "{}", e).ok();
                            msg.draw_border = false;
                            msg.clear_area = false;
                            msg.ellipsis = true;
                            msg.invert = true;
                            self.gfx.draw_textview(&mut msg).unwrap();
                        }
                    }
                }
                let elapsed = Instant::now().duration_since(self.start_time.unwrap_or(Instant::now()));
                if elapsed.as_secs() > FACTORY_TIMEOUT_S {
                    self.factory_test = FactoryTestState::Error("Timeout!".to_string());
                }
                let mut timer = TextView::new(
                    Gid::dummy(),
                    TextBounds::CenteredBot(Rectangle::new(Point::new(32, 115), Point::new(96, 128))),
                );
                timer.style = GlyphStyle::Small;
                write!(timer, "{:.1}s", elapsed.as_secs_f32()).ok();
                timer.draw_border = false;
                timer.clear_area = false;
                timer.ellipsis = false;
                timer.invert = true;
                self.gfx.draw_textview(&mut timer).unwrap();
            }
            VaultMode::Tour => {
                if self.orientation != DisplayOrientation::Normal {
                    self.gfx.bitmap(&bitmaps::badge_flip::BITMAP, None, None).ok();
                } else {
                    match &self.tour_state {
                        TourState::Welcome { seen_press: _ } => {
                            self.gfx.bitmap(&bitmaps::tour_welcome::BITMAP, None, None).ok();
                        }
                        // no diffusion so the menu transition is snappy
                        TourState::LightGeneExplainer1 { seen_press: _ } => {
                            self.gfx.bitmap(&bitmaps::tour_light_gene_explainer1::BITMAP, None, None).ok();
                        }
                        TourState::LightGeneExplainer2 { seen_press: _ } => {
                            self.gfx
                                .bitmap_diffusion(&bitmaps::tour_light_gene_explainer2::BITMAP, None, None)
                                .ok();
                        }
                        TourState::Breeding1 { seen_press: _ } => {
                            self.gfx.bitmap_diffusion(&bitmaps::tour_breeding1::BITMAP, None, None).ok();
                        }
                        TourState::Breeding2 { seen_press: _ } => {
                            self.gfx.bitmap_diffusion(&bitmaps::tour_breeding2::BITMAP, None, None).ok();
                        }
                        TourState::Breeding3 { seen_press: _ } => {
                            self.gfx.bitmap_diffusion(&bitmaps::tour_breeding3::BITMAP, None, None).ok();
                        }
                        TourState::Breeding4 { seen_press: _ } => {
                            self.gfx.bitmap_diffusion(&bitmaps::tour_breeding4::BITMAP, None, None).ok();
                        }
                        TourState::BadgeRecap { seen_press: _ } => {
                            self.gfx.bitmap_diffusion(&bitmaps::tour_recap::BITMAP, None, None).ok();
                        }
                        TourState::TokenIntro1 { seen_press: _ } => {
                            self.gfx.bitmap_diffusion(&bitmaps::tour_token_intro1::BITMAP, None, None).ok();
                        }
                        TourState::TokenIntro2 { seen_press: _ } => {
                            self.gfx.bitmap_diffusion(&bitmaps::tour_token_intro2::BITMAP, None, None).ok();
                        }
                        TourState::TokenIntro3 { seen_press: _ } => {
                            self.gfx.bitmap_diffusion(&bitmaps::tour_token_intro3::BITMAP, None, None).ok();
                        }
                        TourState::InfoScreen { seen_press: _ } => {
                            self.gfx.bitmap_diffusion(&bitmaps::tour_info_screen::BITMAP, None, None).ok();
                        }
                        _ => {}
                    }
                }
            }
            VaultMode::DefconHelp => match &self.help_state {
                HelpState::BadgeRecap { seen_press: _ } => {
                    self.gfx.bitmap(&bitmaps::tour_recap::BITMAP, None, None).ok();
                }
                HelpState::InfoScreen { seen_press: _ } => {
                    self.gfx.bitmap(&bitmaps::tour_info_screen::BITMAP, None, None).ok();
                }
                _ => {}
            },
            VaultMode::TokenHelp => match &self.token_help_state {
                TokenHelpState::TokenRecap { seen_press: _ } => {
                    if self.orientation == DisplayOrientation::Normal {
                        self.gfx.bitmap(&bitmaps::tour_token_recap::BITMAP, None, None).ok();
                    } else {
                        self.gfx.bitmap(&bitmaps::tour_token_recap_flipped::BITMAP, None, None).ok();
                    }
                }
                TokenHelpState::Extension { seen_press: _ } => {
                    self.gfx.bitmap(&bitmaps::tour_browser_extension::BITMAP, None, None).ok();
                }
                TokenHelpState::InfoScreen { seen_press: _ } => {
                    self.gfx.bitmap(&bitmaps::tour_info_screen::BITMAP, None, None).ok();
                }
                _ => {}
            },
            VaultMode::About => match &self.about_state {
                AboutState::Bunnie { seen_press: _ } => {
                    self.gfx.bitmap(&bitmaps::bunnie::BITMAP, None, None).ok();
                }
                AboutState::BaochipLogo { seen_press: _ } => {
                    self.gfx.bitmap(&bitmaps::baochip_about::BITMAP, None, None).ok();
                }
                AboutState::Cheeso { seen_press: _ } => {
                    self.gfx.bitmap(&bitmaps::cheeso::BITMAP, None, None).ok();
                }
                AboutState::Diagnostics { seen_press: _ } => {
                    self.gfx.clear().ok();
                    // print battery voltage here
                    let mut msg = TextView::new(
                        Gid::dummy(),
                        TextBounds::CenteredTop(Rectangle::new(Point::new(0, 0), Point::new(128, 128))),
                    );
                    let voltage_code = self.adc.read_raw(
                        bao1x_hal::udma::AdcSource::Ext(bao1x_hal::udma::AdcExtChannel::Adc3),
                        Some(8),
                    );
                    let vbat_mv =
                        ((bao1x_hal::udma::Adc::raw_to_voltage(voltage_code) * 1000.0f32) / 0.318f32) as u32;
                    writeln!(msg, "~Meditations~").ok();
                    // batt level
                    writeln!(msg, "Batt: {} mV", vbat_mv).ok();
                    // badge type
                    writeln!(
                        msg,
                        "Badge: {:?}",
                        self.global_config.as_ref().unwrap().lock().unwrap().badge_type()
                    )
                    .ok();
                    // k0 check
                    writeln!(msg, "k0: {}", self.global_config.as_ref().unwrap().lock().unwrap().k0_hash())
                        .ok();
                    writeln!(
                        msg,
                        "{}",
                        if self.global_config.as_ref().unwrap().lock().unwrap().is_developer() {
                            "Developer"
                        } else {
                            "Sealed"
                        }
                    )
                    .ok();
                    // USB presence
                    writeln!(
                        msg,
                        "USB {}",
                        if self.global_config.as_ref().unwrap().lock().unwrap().is_plugged_in() {
                            "present"
                        } else {
                            "detached"
                        }
                    )
                    .ok();
                    // version
                    writeln!(msg, "{}", self.tt.get_version()).ok();
                    msg.draw_border = false;
                    msg.clear_area = true;
                    msg.ellipsis = false;
                    msg.invert = true;
                    msg.style = GlyphStyle::Small;
                    self.gfx.draw_textview(&mut msg).unwrap();
                }
                AboutState::InfoScreen { seen_press: _ } => {
                    self.gfx.bitmap(&bitmaps::tour_info_screen::BITMAP, None, None).ok();
                }
                _ => {}
            },
            VaultMode::TokenTour => {
                if self.orientation != DisplayOrientation::Normal {
                    self.gfx.bitmap(&bitmaps::badge_flip::BITMAP, None, None).ok();
                } else {
                    match &self.token_tour_state {
                        TokenTourState::TokenTour1 { seen_press: _ } => {
                            self.gfx.bitmap(&bitmaps::tour_token_tour1::BITMAP, None, None).ok();
                        }
                        TokenTourState::TokenTour2 { seen_press: _ } => {
                            self.gfx.bitmap(&bitmaps::tour_token_tour2::BITMAP, None, None).ok();
                        }
                        TokenTourState::TokenTour3 { seen_press: _ } => {
                            self.gfx.bitmap(&bitmaps::tour_token_tour3::BITMAP, None, None).ok();
                        }
                        TokenTourState::TokenRecap { seen_press: _ } => {
                            self.gfx.bitmap(&bitmaps::tour_token_recap::BITMAP, None, None).ok();
                        }
                        TokenTourState::BrowserExtension { seen_press: _ } => {
                            self.gfx.bitmap(&bitmaps::tour_browser_extension::BITMAP, None, None).ok();
                        }
                        TokenTourState::InfoScreen { seen_press: _ } => {
                            self.gfx.bitmap(&bitmaps::tour_info_screen::BITMAP, None, None).ok();
                        }
                        _ => {}
                    }
                }
            } // _ => unimplemented!(),
        }
        self.gfx.flush().ok();
        self.last_mode = (*self.mode.lock().unwrap()).clone();
    }

    /// Returns `true` if in longpress state. Only call this once per key hit input.
    pub(crate) fn manage_longpress(&mut self) -> bool {
        let now = self.tt.elapsed_ms();
        if now - self.last_key_time > KEYUP_DELAY_MS {
            self.start_hold_time = now;
        }
        self.last_key_time = now;
        now - self.start_hold_time > FAST_SCROLL_DELAY_MS
    }

    pub(crate) fn test_string(&mut self, s: &str) {
        let old =
            std::mem::replace(&mut self.factory_test, FactoryTestState::Error("Transitioning".to_string()));
        self.factory_test = old.handle_input(None, Some(s.to_owned()), None, false);
        self.redraw();
    }

    // Called when the jig indicates that the factory test is ready
    pub(crate) fn jig_ready(&mut self) {
        let old =
            std::mem::replace(&mut self.factory_test, FactoryTestState::Error("Transitioning".to_string()));
        self.factory_test = old.handle_input(None, None, None, true);
        self.redraw();
    }

    pub(crate) fn handle_key(&mut self, k: char) -> Option<char> {
        let mode_at_entry = (*self.mode.lock().unwrap()).clone();
        if k == '🔼' {
            self.orientation = DisplayOrientation::Normal;
            self.redraw();
            if mode_at_entry != VaultMode::StandAloneTest {
                return Some(k);
            }
        } else if k == '🔽' {
            self.orientation = DisplayOrientation::UpsideDown;
            self.redraw();
            if mode_at_entry != VaultMode::StandAloneTest {
                return Some(k);
            }
        }
        log::debug!("handle_key: {:?}", mode_at_entry);
        let filtered_k = match mode_at_entry {
            VaultMode::FactoryTest => {
                let old = std::mem::replace(
                    &mut self.factory_test,
                    FactoryTestState::Error("Transitioning".to_string()),
                );
                self.factory_test = old.handle_input(Some(k), None, None, false);
                // allow scanning in factory mode
                if k == '🔥' { Some(k) } else { None }
            }
            VaultMode::StandAloneTest => {
                let old = std::mem::replace(
                    &mut self.standalone_test,
                    StandAloneTestState::Error("Transitioning".to_string()),
                );
                self.standalone_test = old.handle_input(Some(k), None);
                if self.standalone_test.is_terminal() {
                    *self.mode.lock().unwrap() = VaultMode::Idle;
                }
                // don't allow other buttons in this state as it's confusing
                None
            }
            VaultMode::Tour => {
                let old =
                    std::mem::replace(&mut self.tour_state, TourState::Error("transitioning".to_string()));
                self.tour_state = old.handle_input(k);
                if self.tour_state.is_terminal() {
                    *self.mode.lock().unwrap() = VaultMode::Idle;
                }
                // don't allow scanning to start during the tour
                if k != '🔥' { Some(k) } else { None }
            }
            VaultMode::TokenTour => {
                let old = std::mem::replace(
                    &mut self.token_tour_state,
                    TokenTourState::Error("transitioning".to_string()),
                );
                self.token_tour_state = old.handle_input(k);
                if self.token_tour_state.is_terminal() {
                    *self.mode.lock().unwrap() = VaultMode::Password;
                }
                // don't allow scanning to start during the tour
                if k != '🔥' { Some(k) } else { None }
            }
            VaultMode::DefconHelp => {
                let old =
                    std::mem::replace(&mut self.help_state, HelpState::Error("transitioning".to_string()));
                self.help_state = old.handle_input(k);
                if self.help_state.is_terminal() {
                    *self.mode.lock().unwrap() = VaultMode::Idle;
                }
                // don't allow scanning to start during the tour
                if k != '🔥' { Some(k) } else { None }
            }
            VaultMode::TokenHelp => {
                let old = std::mem::replace(
                    &mut self.token_help_state,
                    TokenHelpState::Error("transitioning".to_string()),
                );
                self.token_help_state = old.handle_input(k);
                if self.token_help_state.is_terminal() {
                    match self.global_config.as_ref().unwrap().lock().unwrap().attachment_state() {
                        AttachState::Unattached => *self.mode.lock().unwrap() = VaultMode::Password,
                        _ => *self.mode.lock().unwrap() = VaultMode::Idle,
                    }
                }
                // don't allow scanning to start during the tour
                if k != '🔥' { Some(k) } else { None }
            }
            VaultMode::About => {
                let old =
                    std::mem::replace(&mut self.about_state, AboutState::Error("transitioning".to_string()));
                self.about_state = old.handle_input(k);
                if self.about_state.is_terminal() {
                    *self.mode.lock().unwrap() = VaultMode::Idle;
                }
                if self.about_state.is_diagnostics() {
                    if k == '↑' {
                        let xns = xous_names::XousNames::new().unwrap();
                        let keystore = keystore::Keystore::new(&xns);
                        keystore.bootwait(Some(false)).unwrap();
                        log::info!("Bootwait secret disable activated");
                    }
                }
                // don't allow scanning to start during the tour
                if k != '🔥' { Some(k) } else { None }
            }
            VaultMode::Password => {
                let increment = if self.manage_longpress() { PAGE_INCREMENT } else { 1 };
                match k {
                    '↑' => {
                        for _ in 0..increment {
                            self.display_list.key_action('↑');
                        }
                    }
                    '↓' => {
                        for _ in 0..increment {
                            self.display_list.key_action('↓');
                        }
                    }
                    '←' => {
                        if let Some(item) = self.get_selected_item() {
                            // print any errors within this function as a panic at this line
                            self.handle_autotype(item.guid, false).unwrap();
                        }
                    }
                    '→' => {
                        {
                            *self.mode.lock().unwrap() = VaultMode::Totp;
                        }
                        // reload DB on mode switch
                        xous::send_message(
                            self.actions_conn,
                            xous::Message::new_blocking_scalar(
                                ActionOp::ReloadDb.to_usize().unwrap(),
                                0,
                                0,
                                0,
                                0,
                            ),
                        )
                        .ok();
                        self.refresh_draw_list();
                    }
                    _ => {
                        // log::warn!("Password unhandled char: {}", k)
                    }
                }
                Some(k)
            }
            VaultMode::Totp => {
                match k {
                    '↑' => {
                        self.display_list.key_action('↑');
                    }
                    '↓' => {
                        self.display_list.key_action('↓');
                    }
                    '←' => {
                        if let Some(code) = self.update_selected_totp_code() {
                            // ignore USB errors while sending code
                            self.usb_dev.send_str(&code).ok();
                        }
                    }
                    '→' => {
                        {
                            // lock needs to go out of scope so we don't hang the later ops
                            *self.mode.lock().unwrap() = VaultMode::Password;
                        }
                        // reload DB on mode switch
                        xous::send_message(
                            self.actions_conn,
                            xous::Message::new_blocking_scalar(
                                ActionOp::ReloadDb.to_usize().unwrap(),
                                0,
                                0,
                                0,
                                0,
                            ),
                        )
                        .ok();
                        self.refresh_draw_list();
                    }
                    _ => {
                        // log::warn!("TOTP unhandled char: {}", k)
                    }
                }
                self.totp_code = None;
                Some(k)
            }
            VaultMode::Idle => match k {
                '←' | '→' => {
                    if let Some(config) = self.global_config.as_mut() {
                        let mut c = config.lock().unwrap();
                        c.lock_rate(); // lock in the mutation rate at this point in time
                        let data = c.nonce_data();
                        let encoded = base45::encode(&data);
                        let code =
                            QrCode::with_error_correction_level(encoded.as_bytes(), qrcode::EcLevel::M)
                                .expect("couldn't build QR code");
                        log::info!(
                            "Nonce encoded {} bytes to Qrcode version {:?}",
                            encoded.as_bytes().len(),
                            code.version()
                        );

                        self.qr_override = Some(code);
                        {
                            *self.mode.lock().unwrap() = VaultMode::ShowKey { quantum: 0 };
                        }
                    }
                    None
                }
                '🔥' => {
                    *self.mode.lock().unwrap() = VaultMode::GeneScan;
                    Some(k)
                }
                _ => Some(k),
            },
            VaultMode::IdleDevMode => Some(k),
            VaultMode::ShowKey { quantum: _ } => {
                match k {
                    '🔥' => {
                        *self.mode.lock().unwrap() = VaultMode::GeneScan;
                        // pass on the "fire" key to activate QR scanning
                        Some('🔥')
                    }
                    _ => {
                        // cancel out and return to idle screen
                        *self.mode.lock().unwrap() = VaultMode::Idle;
                        Some(k)
                    }
                }
            }
            // this is the next state of the recipient after showing the key
            VaultMode::ConfirmGene => Some(k),
            // this is the state of the donor in response to query
            VaultMode::ResponseGene { quantum: _ } => {
                self.global_config.as_mut().unwrap().lock().unwrap().clear_nonces();
                *self.mode.lock().unwrap() = VaultMode::Idle;
                // eat the 'fire' button if it's pressed - we just want to go back to the idle
                // screen in all button presses
                if k != '🔥' { Some(k) } else { None }
            }
            // catch-all for now
            _ => Some(k),
        };
        self.animate.store(self.mode.lock().unwrap().should_animate(), Ordering::SeqCst);
        // don't redraw if menu is being raised
        if k != '∴' {
            self.redraw();
        }
        filtered_k
    }

    pub(crate) fn camera_transition(&mut self) {
        self.gfx.clear().ok();
        let mut tv = TextView::new(
            Gid::dummy(),
            TextBounds::CenteredTop(Rectangle::new(Point::new(0, 52), Point::new(127, 96))),
        );
        tv.invert = true;
        tv.margin = Point::new(2, 2);
        tv.style = GlyphStyle::Bold;
        tv.draw_border = false;
        write!(tv, "Starting\ncamera...").ok();
        self.gfx.draw_textview(&mut tv).ok();
        self.redraw();
    }

    pub(crate) fn filter(&mut self, criteria: &String) {
        self.filter = criteria.to_owned();
        // only filter passwords in this implementation
        if self.filter.is_empty() {
            self.item_lists.lock().unwrap().filter_reset(VaultMode::Password);
        } else {
            self.item_lists.lock().unwrap().filter_reset(VaultMode::Password);
            self.item_lists.lock().unwrap().filter(VaultMode::Password, criteria);
        }
    }

    pub(crate) fn get_filter(&self) -> String { self.filter.to_owned() }

    pub(crate) fn handle_autotype(&mut self, guid: String, type_username: bool) -> Result<(), String> {
        // we re-fetch the entry for autotype, because the PDDB could have unmounted a basis.
        let atime = utc_now().timestamp() as u64;
        let pddb_binding = self.pddb.borrow();

        let mut record = pddb_binding
            .get(
                dc34_vault::VAULT_PASSWORD_DICT,
                &guid,
                None,
                false,
                false,
                None,
                Some(dc34_vault::basis_change),
            )
            .map_err(|e| format!("couldn't access key {}: {:?}", guid, e))?;
        let mut data = Vec::<u8>::new();
        record.read_to_end(&mut data).map_err(|_| format!("Couldn't access key {}", guid))?;
        let mut pw = crate::storage::PasswordRecord::try_from(data)
            .map_err(|_| format!("Couldn't deserialize {}", guid))?;
        let to_type = if type_username { &pw.username } else { &pw.password };
        self.usb_dev.send_str(to_type).ok(); // ignore USB errors
        pw.count += 1;
        pw.atime = atime;

        // this get determines which basis the key is in
        let app_data = pddb_binding
            .get(
                dc34_vault::VAULT_PASSWORD_DICT,
                &guid,
                None,
                true,
                true,
                Some(256),
                Some(dc34_vault::basis_change),
            )
            .map_err(|e| format!("error updating key atime: {:?}", e))?;
        let basis = app_data.attributes().map_err(|_| "couldn't get attributes")?.basis;

        // delete the old key
        pddb_binding
            .delete_key(dc34_vault::VAULT_PASSWORD_DICT, &guid, Some(&basis))
            .map_err(|_| "Couldn't delete previous pw entry")?;

        // write the new key in
        let mut record = pddb_binding
            .get(
                dc34_vault::VAULT_PASSWORD_DICT,
                &guid,
                Some(&basis),
                false,
                true,
                Some(dc34_vault::VAULT_ALLOC_HINT),
                Some(dc34_vault::basis_change),
            )
            .map_err(|e| format!("couldn't update key {}: {:?}", guid, e))?;
        let ser: Vec<u8> = crate::storage::PasswordRecord::into(pw);
        record.write(&ser).map_err(|e| format!("couldn't update key {}: {:?}", guid, e))?;

        self.pddb.borrow().sync().ok();
        Ok(())
    }
}
