//! The extra rows in the Paint screen's per-part list, after WEATHERING. A row is one value set for the whole AC, never
//! per part: a color, and optionally a choice from a list and more colors, which its panel (panel.rs) offers. How
//! the Paint screen builds its list and panels, and what each hook is for: docs/paint-ui.md; storage: docs/storage.md.
//! The rows' clips are in the Paint movie (tools/build_gfx.py; its row count matches ROW_COUNT).
//!
//! The rows are items of ours, appended to the list the game's row builder made, built as it builds its color rows:
//! our label, our kind (a value no stock row has), our refresh (the swatch) and the color rows' on-confirm (into the
//! current panel, which the panel switch builds for our kind).
//!
//! Hooks:
//! - 0x140f418d0 row builder: keeps the edit context it is given while it runs, for the hand-off below.
//! - 0x140d3cb00 list -> list widget: the row builder's call gets our rows appended first.
//! - 0x140f43610 row kind -> panel: our rows and DEVICE get the panel panel.rs builds.
//! - 0x14045de80 is-normal-slot: DEVICE is (so its stock row leads to a panel).

use std::sync::OnceLock;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::panel::{self, DEVICE_SLOT};
use crate::game::{self, leak_utf16};
use crate::ui::{self, Closure, Function, FunctionTable, Label};
use crate::{Hook, log, orig};

// The rows, top to bottom.
pub(crate) const ROW_THRUSTER: usize = 0;
pub(crate) const ROW_HUD: usize = 1;
pub(crate) const ROW_WEAPONS: usize = 2;
const ROW_COUNT: usize = 3;
// A row's kind is FIRST_KIND + its index: past the stock ones (0-5 the color slots, 6 COLOR SET, 7 PATTERN,
// 8 WEATHERING).
const FIRST_KIND: u32 = 9;
// Where a row keeps its values, whichever part is selected (docs/storage.md): a paint section no part uses, its
// colors from one of the section's colors on, its choice in one of its materials. THRUSTERS and HUD share section 7;
// WEAPON VFX (a dynamic row) says where each entry's color is itself: the weapons' in sections 9 and 6, the
// expansion's in section 7's third color.
struct Storage {
    section: usize,
    first_color: usize,
    choice_material: usize,
}
static STORAGE: [Storage; ROW_COUNT] = [
    Storage { section: 7, first_color: 0, choice_material: 0 },
    Storage { section: 7, first_color: 3, choice_material: 1 },
    Storage { section: 9, first_color: 0, choice_material: 0 },
];
// A paint section: its six colors (u32s of RGBA bytes), its six materials (u16s), ..., its four pattern colors.
const SECTION_SIZE: usize = 0x44;
const SECTION_COLORS_OFFSET: usize = 8;
const SECTION_MATERIALS_OFFSET: usize = 0x20;
const SECTION_PATTERN_COLORS_OFFSET: usize = 0x30;
const SECTION_COLORS: usize = 6;

/// Alpha byte that marks a stored color as picked in its row. The game writes 0xFF into every color it stores (the
/// unused sections hold its default grey, older saves can have black with 0), so 0xFE only ever comes from us. It
/// travels with the color: saved with the AC and carried by designs.
const PICKED_ALPHA: u32 = 0xFE00_0000;
const ALPHA_MASK: u32 = 0xFF00_0000;
// A row's colors at most: its own and two extra.
const MAX_COLORS: usize = 3;
/// The same for a stored choice: 0xFE00 + the choice's index (the unused sections hold the default material, 18).
const PICKED_CHOICE: u16 = 0xFE00;
const CHOICE_MASK: u16 = 0x00FF;

/// A claimed row. Texts are UTF-16 with a terminator, never freed (the game's labels keep their addresses).
struct Row {
    label: &'static [u16],
    /// The color's name in the panel, if not the game's "Color" (the picker's title then; else the row's label).
    color_name: Option<&'static [u16]>,
    /// Defaults as stored (RGBA bytes), for a color the player hasn't picked: the row's color, then its extra colors.
    default_colors: [u32; MAX_COLORS],
    /// The extra colors' names (the second color, the third).
    extra_colors: &'static [&'static [u16]],
    choices: Option<Choices>,
    /// Labels decided when the panel is built (claim_dynamic): a color's, by which.
    labels: Option<fn(usize) -> &'static [u16]>,
    /// Where an entry's color is (claim_dynamic): see Slot.
    slot: Option<fn(usize) -> Option<Slot>>,
}

/// Where a dynamic row's entry keeps its color right now: its offset in the paint block (a color field, color_field),
/// and its default (0xRRGGBB).
#[derive(Clone, Copy)]
pub(crate) struct Slot {
    pub(crate) offset: usize,
    pub(crate) default: u32,
}

struct Choices {
    /// The choice entry's header and the option list's title.
    title: &'static [u16],
    labels: &'static [&'static [u16]],
    default: u32,
}

static ROWS: [OnceLock<Row>; ROW_COUNT] = [const { OnceLock::new() }; ROW_COUNT];

// The edit context the row builder was given, while it runs (menu code runs on one thread); 0 otherwise.
static BUILDER_CONTEXT: AtomicUsize = AtomicUsize::new(0);
const ITEM_FLAG_PANEL: u8 = 8;
// The Paint list as the builder makes it: COLOR SET, the six colors, PATTERN, WEATHERING (kind 8) last.
const STOCK_ROW_COUNT: usize = 9;
const WEATHERING_KIND: i32 = 8;

static ORIG_IS_NORMAL_SLOT: AtomicUsize = AtomicUsize::new(0);
static ORIG_BUILD_ROWS: AtomicUsize = AtomicUsize::new(0);
static ORIG_MAKE_LIST: AtomicUsize = AtomicUsize::new(0);
static ORIG_MAKE_PANEL: AtomicUsize = AtomicUsize::new(0);

// ---- the rows' settings ----------------------------------------------------------------------------------------------

fn settings(row: usize) -> Option<&'static Row> {
    ROWS.get(row)?.get()
}

/// The claimed rows, top to bottom.
fn claimed_rows() -> impl Iterator<Item = (usize, &'static Row)> {
    (0..ROW_COUNT).filter_map(|row| Some((row, settings(row)?)))
}

/// The claimed row with this kind.
pub(crate) fn row_of_kind(kind: u32) -> Option<usize> {
    let row = kind.checked_sub(FIRST_KIND)? as usize;
    settings(row).map(|_| row)
}

/// 0xRRGGBB <-> the game's stored color (RGBA bytes; alpha left 0 / dropped).
fn swap_red_blue(color: u32) -> u32 {
    let [b, g, r, _] = color.to_le_bytes();
    u32::from_le_bytes([r, g, b, 0])
}

fn stored_default(color: u32) -> u32 {
    swap_red_blue(color) | ALPHA_MASK
}

/// Shows a row in the Paint list (before the hooks go live: claimed once), with a label, showing `color` (0xRRGGBB)
/// until the player picks one. Its panel offers that color (named `color_name`, else the game's "Color"), a
/// `extra_colors` (label, default each) and `choices` (title, labels, the default one): the panel's entries are the
/// color, the second color, the choices, the third color, the order of the panel clip's color, color, material and
/// color items (tools/build_gfx.py), so choices need a second color and a third color needs choices.
pub(crate) fn claim(
    row: usize,
    label: &str,
    (color_name, color): (Option<&str>, u32),
    extra_colors: &[(&str, u32)],
    choices: Option<(&str, &[&str], u32)>,
) {
    assert!(extra_colors.len() < MAX_COLORS);
    assert!(choices.is_none() || !extra_colors.is_empty());
    assert!(extra_colors.len() < 2 || choices.is_some());
    let choices = choices.map(|(title, labels, default)| {
        assert!(!labels.is_empty() && labels.len() <= CHOICE_MASK as usize && (default as usize) < labels.len());
        let labels = labels.iter().map(|label| leak_utf16(label)).collect::<Vec<_>>().leak();
        Choices { title: leak_utf16(title), labels, default }
    });
    let settings = Row {
        label: leak_utf16(label),
        color_name: color_name.map(leak_utf16),
        default_colors: std::array::from_fn(|which| match which {
            0 => stored_default(color),
            _ => extra_colors.get(which - 1).map_or(ALPHA_MASK, |&(_, color)| stored_default(color)),
        }),
        extra_colors: extra_colors.iter().map(|&(label, _)| leak_utf16(label)).collect::<Vec<_>>().leak(),
        choices,
        labels: None,
        slot: None,
    };
    assert!(ROWS[row].set(settings).is_ok(), "row {row} claimed twice");
}

/// A row of `count` colors (1 to 3, no choices) whose entries are decided when its panel is built: their labels
/// (`labels(which)`, kept forever) and where their colors are (`slot(which)`; None: nothing to edit, shown white).
/// Lets a row keep more colors in its section than its panel shows at once.
pub(crate) fn claim_dynamic(row: usize, label: &str, count: usize, labels: fn(usize) -> &'static [u16],
                            slot: fn(usize) -> Option<Slot>) {
    assert!((1..=MAX_COLORS).contains(&count));
    let settings = Row {
        label: leak_utf16(label),
        color_name: None,
        default_colors: [ALPHA_MASK; MAX_COLORS],
        // The extra colors' labels come from `labels`; only their count matters here.
        extra_colors: vec![&[0u16][..]; count - 1].leak(),
        choices: None,
        labels: Some(labels),
        slot: Some(slot),
    };
    assert!(ROWS[row].set(settings).is_ok(), "row {row} claimed twice");
}

fn color_offset(row: usize, which: usize) -> usize {
    color_field(STORAGE[row].section, STORAGE[row].first_color + which)
}

fn choice_offset(row: usize) -> usize {
    STORAGE[row].section * SECTION_SIZE + SECTION_MATERIALS_OFFSET + STORAGE[row].choice_material * 2
}

/// Where a section's color field `field` is in the paint block: its six colors (0-5), then its four pattern colors
/// (6-9; no part uses them in the sections the rows use).
pub(crate) fn color_field(section: usize, field: usize) -> usize {
    assert!(field < 10);
    section * SECTION_SIZE + match field {
        0..SECTION_COLORS => SECTION_COLORS_OFFSET + field * 4,
        _ => SECTION_PATTERN_COLORS_OFFSET + (field - SECTION_COLORS) * 4,
    }
}

/// Where an entry's color is, and its default as stored: a dynamic row's slot, else its color field.
fn entry(settings: &Row, row: usize, which: usize) -> Option<(usize, u32)> {
    match settings.slot {
        Some(slot) => slot(which).map(|slot| (slot.offset, stored_default(slot.default))),
        None => Some((color_offset(row, which), settings.default_colors[which])),
    }
}

/// A color shown for a stored value, opaque: the stored one if it was picked in the row, otherwise the default.
fn shown_color(settings: &Row, which: usize, stored: u32) -> u32 {
    if stored & ALPHA_MASK == PICKED_ALPHA { stored | ALPHA_MASK } else { settings.default_colors[which] }
}

/// A choice shown for a stored value: the stored one if it was picked in the row and still exists, otherwise the
/// default.
fn shown_choice(choices: &Choices, stored: u16) -> u32 {
    let index = (stored & CHOICE_MASK) as u32;
    if stored & !CHOICE_MASK == PICKED_CHOICE && (index as usize) < choices.labels.len() { index } else { choices.default }
}

// ---- on the player's AC ----------------------------------------------------------------------------------------------

/// A row's color (`which`: 0 its color, 1 and 2 its extra ones) on the player's AC (0xRRGGBB): the picked one, or the
/// default. None while the player's data isn't loaded.
pub(crate) fn color(row: usize, which: usize) -> Option<u32> {
    Some(swap_red_blue(shown_color(settings(row)?, which, game::paint_value(color_offset(row, which))?)))
}

/// A color picked in a dynamic row, at `offset` in the player's paint block (0xRRGGBB): None if not picked or the
/// player's data isn't loaded.
pub(crate) fn picked_at(offset: usize) -> Option<u32> {
    let stored = game::paint_value::<u32>(offset)?;
    (stored & ALPHA_MASK == PICKED_ALPHA).then(|| swap_red_blue(stored))
}

/// A row's choice on the player's AC, as an index into its labels: the picked one, or the default. None if it has no
/// choices or the player's data isn't loaded.
pub(crate) fn choice(row: usize) -> Option<u32> {
    let choices = settings(row)?.choices.as_ref()?;
    Some(shown_choice(choices, game::paint_value(choice_offset(row))?))
}

// ---- for the panel (panel.rs): in a paint block, the edit context's working one -------------------------------------

/// A row's color (`which`: 0 the row's, 1 and 2 its extra ones) as stored (RGBA bytes, alpha 0xFF).
pub(crate) unsafe fn stored_color(paint: usize, row: usize, which: usize) -> u32 {
    let Some((offset, default)) = settings(row).and_then(|s| entry(s, row, which)) else { return u32::MAX };
    let stored = unsafe { *((paint + offset) as *const u32) };
    if stored & ALPHA_MASK == PICKED_ALPHA { stored | ALPHA_MASK } else { default }
}

pub(crate) unsafe fn store_color(paint: usize, row: usize, which: usize, color: u32) {
    let Some((offset, _)) = settings(row).and_then(|s| entry(s, row, which)) else { return };
    unsafe { *((paint + offset) as *mut u32) = (color & !ALPHA_MASK) | PICKED_ALPHA };
}

/// A color's name, as the picker's title: the row's color name (else its label), an extra color's.
pub(crate) fn color_label(row: usize, which: usize) -> &'static [u16] {
    let Some(settings) = settings(row) else { return &[0] };
    if let Some(labels) = settings.labels {
        return labels(which);
    }
    match which {
        0 => settings.color_name.unwrap_or(settings.label),
        _ => settings.extra_colors.get(which - 1).copied().unwrap_or(&[0]),
    }
}

/// The row's color's name in the panel, if it has its own.
pub(crate) fn color_name(row: usize) -> Option<&'static [u16]> {
    let settings = settings(row)?;
    settings.labels.map(|labels| labels(0)).or(settings.color_name)
}

/// How many colors a row has besides its own.
pub(crate) fn extra_color_count(row: usize) -> usize {
    settings(row).map_or(0, |s| s.extra_colors.len())
}

/// A row's choices: their title and labels (none if it has no choices).
pub(crate) fn choices(row: usize) -> Option<(&'static [u16], &'static [&'static [u16]])> {
    settings(row)?.choices.as_ref().map(|choices| (choices.title, choices.labels))
}

/// A row's choice, as an index.
pub(crate) unsafe fn stored_choice(paint: usize, row: usize) -> u32 {
    let Some(choices) = settings(row).and_then(|s| s.choices.as_ref()) else { return 0 };
    shown_choice(choices, unsafe { *((paint + choice_offset(row)) as *const u16) })
}

pub(crate) unsafe fn store_choice(paint: usize, row: usize, index: u32) {
    unsafe { *((paint + choice_offset(row)) as *mut u16) = PICKED_CHOICE | index as u16 };
}

// ---- the rows' items -------------------------------------------------------------------------------------------------

/// What a row's refresh captures: the edit context (holding a reference on it) and the row, as a stock color row's
/// refresh captures the context and its slot (0x140f3f3a0).
#[derive(Clone, Copy)]
struct Swatch {
    context: usize,
    row: usize,
}

impl ui::HoldsReference for Swatch {
    fn referenced(&self) -> usize {
        self.context
    }
}

static ITEM_REFRESH: FunctionTable = ui::table_holding::<Swatch>(item_refresh as *const ());
static ITEM_CONFIRM: FunctionTable = ui::table::<()>(item_confirm as *const ());
// The game's own job body for it: {table, Paint screen} -> confirm sound, the current panel's cursor, the panel pushed
// on the screen's child windows (0x140f43d90, closure table 0x143986878).
static ENTER_PANEL: FunctionTable = ui::table::<usize>(0x140f43d90 as *const ());

/// The swatch, from the working paint block (so it follows the picker).
unsafe extern "system" fn item_refresh(this: *const Closure<Swatch>, clip: usize) {
    let swatch = unsafe { (*this).captures };
    unsafe {
        let color = stored_color(ui::context_paint(swatch.context), swatch.row, 0);
        ui::show_swatch(clip, color, ui::context_category(swatch.context));
    }
}

/// A job moving into the current panel, as a stock color row's on-confirm returns: its closure captures nothing and
/// passes on the window it's called with, the Paint screen (0x140f43e30 -> ... -> 0x140f43500(window)).
unsafe extern "system" fn item_confirm(_this: *const Closure<()>, out: *mut usize, window: usize) -> *mut usize {
    let mut enter = Function::new();
    unsafe { ui::function_job(out, enter.set(Closure { table: &ENTER_PANEL, captures: window })) };
    out
}

/// Appends the claimed rows to the Paint list's description, after WEATHERING.
unsafe fn append_rows(description: usize, context: usize) {
    let (start, end) = unsafe { ui::items(description) };
    let count = (end - start) / ui::ITEM_SIZE;
    if count != STOCK_ROW_COUNT || unsafe { *((end - ui::ITEM_SIZE + ui::ITEM_KIND_OFFSET) as *const i32) } != WEATHERING_KIND {
        log(&format!("a list of {count} rows inside the row builder isn't the Paint list as built: no rows added"));
        return;
    }
    for (row, settings) in claimed_rows() {
        let mut label = Label::new();
        let mut confirm = Function::new();
        unsafe {
            let item = ui::add_item(description, label.set(settings.label), confirm.set(Closure { table: &ITEM_CONFIRM, captures: () }));
            *((item + ui::ITEM_KIND_OFFSET) as *mut u32) = FIRST_KIND + row as u32;
            // The refresh's own reference on the context, released when the item's function is destroyed.
            ui::add_reference(context);
            ui::set_function(item + ui::ITEM_REFRESH_OFFSET, Closure { table: &ITEM_REFRESH, captures: Swatch { context, row } });
            // As the builder does to the rows that open a panel (0x140f41f20): without it the list doesn't grey out
            // around the row while its panel has the focus.
            *((item + ui::ITEM_FLAGS_OFFSET) as *mut u8) |= ITEM_FLAG_PANEL;
        }
    }
}

// ---- hooks -----------------------------------------------------------------------------------------------------------

type BuildRowsFn = unsafe extern "system" fn(usize, usize, usize, usize) -> usize;

// (out, screen, the edit context's holder, a clip): the third is what the stock rows' refresh closures take their edit
// context from (`*context`, with a reference added).
unsafe extern "system" fn build_rows_impl(out: usize, screen: usize, context: *const usize, a4: usize) -> usize {
    BUILDER_CONTEXT.store(unsafe { *context }, Ordering::Relaxed);
    let result = unsafe { orig::<BuildRowsFn>(&ORIG_BUILD_ROWS)(out, screen, context as usize, a4) };
    BUILDER_CONTEXT.store(0, Ordering::Relaxed);
    result
}

type MakeListFn = unsafe extern "system" fn(usize, usize, usize, usize) -> usize;

// The row builder hands its finished description to this, which copies it into the list widget.
unsafe extern "system" fn make_list_impl(description: usize, out: usize, a3: usize, a4: usize) -> usize {
    let context = BUILDER_CONTEXT.load(Ordering::Relaxed);
    if context != 0 {
        unsafe { append_rows(description, context) };
    }
    unsafe { orig::<MakeListFn>(&ORIG_MAKE_LIST)(description, out, a3, a4) }
}

unsafe extern "system" fn is_normal_slot_impl(slot: *const u8) -> bool {
    unsafe { *slot == DEVICE_SLOT || orig::<unsafe extern "system" fn(*const u8) -> bool>(&ORIG_IS_NORMAL_SLOT)(slot) }
}

type MakePanelFn = unsafe extern "system" fn(usize, *mut usize, u32) -> *mut usize;

// The game's switch has cases for the color slots' kinds only. A panel has a reference for the list, as the cases add.
unsafe extern "system" fn make_panel_impl(list: usize, out: *mut usize, kind: u32) -> *mut usize {
    let row = row_of_kind(kind);
    if row.is_none() && kind != DEVICE_SLOT as u32 {
        return unsafe { orig::<MakePanelFn>(&ORIG_MAKE_PANEL)(list, out, kind) };
    }
    unsafe {
        let panel = panel::build(list, row);
        *out = panel;
        if panel != 0 {
            ui::add_reference(panel);
        }
    }
    out
}

/// The hooks, for lib.rs's hook table.
pub(crate) fn hooks() -> [Hook; 4] {
    [
        // jmp (the function starts elsewhere)
        (0x140f418d0, [0xe9, 0x83, 0x90, 0x55, 0x05, 0x48], build_rows_impl as *const (), &ORIG_BUILD_ROWS),
        // push rbp / push rbx / push rsi / push rdi / push r14
        (0x140d3cb00, [0x40, 0x55, 0x53, 0x56, 0x57, 0x41], make_list_impl as *const (), &ORIG_MAKE_LIST),
        // mov rax,rsp / mov [rax+10],rdx
        (0x140f43610, [0x48, 0x8b, 0xc4, 0x48, 0x89, 0x50], make_panel_impl as *const (), &ORIG_MAKE_PANEL),
        // movzx eax,byte [rcx] / cmp al,5
        (0x14045de80, [0x0f, 0xb6, 0x01, 0x3c, 0x05, 0x74], is_normal_slot_impl as *const (), &ORIG_IS_NORMAL_SLOT),
    ]
}
