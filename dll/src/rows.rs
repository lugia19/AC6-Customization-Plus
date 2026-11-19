//! The extra rows in the Paint screen's per-part list, after WEATHERING. A row is one value set for the whole AC, never
//! per part: a color, and optionally a choice from a list and a second color, which its panel (panel.rs) offers. How
//! the Paint screen builds its list and panels, and what each hook is for: docs/paint-rows.md. The rows' clips are in
//! the Paint movie (tools/build_gfx.py).
//!
//! The rows are items of ours, appended to the list the game's row builder made: our label, our kind (a value no
//! stock row has), our refresh (the swatch) and our on-confirm (into the row's panel, as a stock color row does).
//!
//! Hooks:
//! - 0x140f418d0 row builder: marks when it's running (and keeps the Paint screen), for the hand-off below.
//! - 0x140d3cb00 list -> list widget: the row builder's call gets our rows appended first.
//! - 0x140f43610 row kind -> panel: our rows and DEVICE get the panel panel.rs builds.
//! - 0x14045de80 is-normal-slot: DEVICE is (so its stock row leads to a panel; device.rs).

use std::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};

use crate::ui::{self, Closure, Function, FunctionTable, Label, leak_utf16, with_clip};
use crate::{Hook, device, log, orig, paint_data, panel};

// The rows, top to bottom (row 2 is free).
pub(crate) const ROW_THRUSTER: usize = 0;
pub(crate) const ROW_HUD: usize = 1;
const ROW_COUNT: usize = 3;
// The rows' kinds: past the stock ones (0-5 the color slots, 6 COLOR SET, 7 PATTERN, 8 WEATHERING).
static KINDS: [i32; ROW_COUNT] = [9, 10, 11];
// The paint section a row keeps its values in, whichever part is selected: its colors in colors 0 and 1, its choice
// in material 0. No part uses these sections. Row 0 is on 7 because version 0.1.0 kept the thruster color there.
static SECTIONS: [u8; ROW_COUNT] = [7, 6, 9];
// A paint section: its six colors (u32s of RGBA bytes), its six materials (u16s).
const SECTION_SIZE: usize = 0x44;
const SECTION_COLORS_OFFSET: usize = 8;
const SECTION_MATERIALS_OFFSET: usize = 0x20;
const CHOICE_STORAGE: usize = 0;

/// Alpha byte that marks a stored color as picked in its row. The game writes 0xFF into every color it stores (the
/// unused sections hold its default grey, older saves can have black with 0), so 0xFE only ever comes from us. It
/// travels with the color: saved with the AC and carried by designs.
const PICKED_ALPHA: u32 = 0xFE00_0000;
const ALPHA_MASK: u32 = 0xFF00_0000;
/// The same for a stored choice: 0xFE00 + the choice's index (the unused sections hold the default material, 18).
const PICKED_CHOICE: u16 = 0xFE00;
const CHOICE_MASK: u16 = 0x00FF;

/// A claimed row. Texts are UTF-16 with a terminator; they and the rows are never freed (the game's labels keep the
/// texts' addresses).
#[derive(Clone, Copy)]
struct Row {
    label: &'static [u16],
    /// Defaults as stored (RGBA bytes), for a color the player hasn't picked: the row's color, its second color.
    default_colors: [u32; 2],
    /// The second color's name, if the row has one.
    second_color: Option<&'static [u16]>,
    choices: Option<Choices>,
}

#[derive(Clone, Copy)]
struct Choices {
    /// The choice entry's header and the option list's title.
    title: &'static [u16],
    labels: &'static [&'static [u16]],
    default: u32,
}

static ROWS: [AtomicPtr<Row>; ROW_COUNT] = [const { AtomicPtr::new(std::ptr::null_mut()) }; ROW_COUNT];

// The edit context the row builder was given, while it runs (menu code runs on one thread); 0 otherwise.
static BUILDER_CONTEXT: AtomicUsize = AtomicUsize::new(0);
// The Paint screen: its panel state (starts with the screen itself), the current panel. The edit context: its
// working paint block.
const SCREEN_PANELS_OFFSET: usize = 0x2618;
const SCREEN_CURRENT_PANEL_OFFSET: usize = 0x2758;
const EDIT_CONTEXT_CATEGORY_OFFSET: usize = 0x28;
const EDIT_CONTEXT_PAINT_OFFSET: usize = 0x40;
// The description's items (0x308 bytes each), and an item's kind.
const DESCRIPTION_ITEMS_OFFSET: usize = 0xc8;
const DESCRIPTION_ITEMS_END_OFFSET: usize = 0xd0;
const ITEM_SIZE: usize = 0x308;
const ITEM_KIND_OFFSET: usize = 0x98;
const ITEM_FLAGS_OFFSET: usize = 0x300;
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
    unsafe { ROWS.get(row)?.load(Ordering::Relaxed).as_ref() }
}

fn update(row: usize, change: impl FnOnce(&mut Row)) {
    let Some(&old) = settings(row) else { return };
    let mut new = old;
    change(&mut new);
    ROWS[row].store(Box::leak(Box::new(new)), Ordering::Relaxed);
}

fn claimed_rows() -> impl Iterator<Item = usize> {
    (0..ROW_COUNT).filter(|&row| settings(row).is_some())
}

/// The claimed row with this kind.
fn row_of_kind(kind: u32) -> Option<usize> {
    let row = KINDS.iter().position(|&k| k as u32 == kind)?;
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

/// Shows a row in the Paint list, with a label, showing `default_color` (0xRRGGBB) until the player picks a color.
pub(crate) fn claim(row: usize, label: &str, default_color: u32) {
    let settings = Row { label: leak_utf16(label), default_colors: [stored_default(default_color), ALPHA_MASK], second_color: None, choices: None };
    ROWS[row].store(Box::leak(Box::new(settings)), Ordering::Relaxed);
}

/// Gives a claimed row a list of choices, under `title`, in its panel. `default` is the one a row starts on.
pub(crate) fn set_choices(row: usize, title: &str, labels: &[&str], default: u32) {
    assert!(!labels.is_empty() && labels.len() <= CHOICE_MASK as usize && (default as usize) < labels.len());
    let labels = labels.iter().map(|label| leak_utf16(label)).collect::<Vec<_>>().leak();
    update(row, |r| r.choices = Some(Choices { title: leak_utf16(title), labels, default }));
}

/// Gives a claimed row a second color in its panel, `label`, showing `default_color` (0xRRGGBB) until picked.
pub(crate) fn set_second_color(row: usize, label: &str, default_color: u32) {
    update(row, |r| {
        r.second_color = Some(leak_utf16(label));
        r.default_colors[1] = stored_default(default_color);
    });
}

fn color_offset(row: usize, which: usize) -> usize {
    SECTIONS[row] as usize * SECTION_SIZE + SECTION_COLORS_OFFSET + which * 4
}

fn choice_offset(row: usize) -> usize {
    SECTIONS[row] as usize * SECTION_SIZE + SECTION_MATERIALS_OFFSET + CHOICE_STORAGE * 2
}

/// A color shown for a stored value, opaque: the stored one if it was picked in the row, otherwise the default.
fn shown_color(settings: &Row, which: usize, stored: u32) -> u32 {
    if stored & ALPHA_MASK == PICKED_ALPHA { stored | ALPHA_MASK } else { settings.default_colors[which] }
}

fn picked(color: u32) -> u32 {
    (color & !ALPHA_MASK) | PICKED_ALPHA
}

/// A choice shown for a stored value: the stored one if it was picked in the row and still exists, otherwise the
/// default.
fn shown_choice(choices: &Choices, stored: u16) -> u32 {
    let index = (stored & CHOICE_MASK) as u32;
    if stored & !CHOICE_MASK == PICKED_CHOICE && (index as usize) < choices.labels.len() { index } else { choices.default }
}

// ---- on the player's AC ----------------------------------------------------------------------------------------------

/// A row's color on the player's AC (0xRRGGBB): the picked one, or the default. None while the player's data isn't
/// loaded.
pub(crate) fn color(row: usize) -> Option<u32> {
    Some(swap_red_blue(shown_color(settings(row)?, 0, paint_data::value(color_offset(row, 0))?)))
}

/// A row's second color on the player's AC (0xRRGGBB), as `color`.
pub(crate) fn second_color(row: usize) -> Option<u32> {
    Some(swap_red_blue(shown_color(settings(row)?, 1, paint_data::value(color_offset(row, 1))?)))
}

/// A row's choice on the player's AC, as an index into its labels: the picked one, or the default. None if it has no
/// choices or the player's data isn't loaded.
pub(crate) fn choice(row: usize) -> Option<u32> {
    let choices = settings(row)?.choices.as_ref()?;
    Some(shown_choice(choices, paint_data::value(choice_offset(row))?))
}

// ---- for the panel (panel.rs): in a paint block, the edit context's working one -------------------------------------

/// A row's color (`which`: 0 the row's, 1 its second) as stored (RGBA bytes, alpha 0xFF).
pub(crate) unsafe fn stored_color(paint: usize, row: usize, which: usize) -> u32 {
    let Some(settings) = settings(row) else { return ALPHA_MASK };
    shown_color(settings, which, unsafe { *((paint + color_offset(row, which)) as *const u32) })
}

pub(crate) unsafe fn store_color(paint: usize, row: usize, which: usize, color: u32) {
    unsafe { *((paint + color_offset(row, which)) as *mut u32) = picked(color) };
}

/// A color's name: the row's label for its color, the second color's for that.
pub(crate) fn color_label(row: usize, which: usize) -> &'static [u16] {
    let Some(settings) = settings(row) else { return &[0] };
    if which == 0 { settings.label } else { settings.second_color.unwrap_or(&[0]) }
}

pub(crate) fn has_second_color(row: usize) -> bool {
    settings(row).is_some_and(|s| s.second_color.is_some())
}

/// A row's choices: their title and labels (none if it has no choices).
pub(crate) fn choices(row: usize) -> Option<(&'static [u16], Vec<&'static [u16]>)> {
    settings(row)?.choices.map(|choices| (choices.title, choices.labels.to_vec()))
}

/// A row's choice, as an index.
pub(crate) unsafe fn stored_choice(paint: usize, row: usize) -> u32 {
    let Some(choices) = settings(row).and_then(|s| s.choices) else { return 0 };
    shown_choice(&choices, unsafe { *((paint + choice_offset(row)) as *const u16) })
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
static ENTER_PANEL: FunctionTable = ui::table::<usize>(enter_panel as *const ());

/// The swatch, from the working paint block (so it follows the picker), with no pattern over it.
unsafe extern "system" fn item_refresh(this: *const Closure<Swatch>, clip: usize) {
    let swatch = unsafe { (*this).captures };
    unsafe {
        let color = stored_color(*((swatch.context + EDIT_CONTEXT_PAINT_OFFSET) as *const usize), swatch.row, 0);
        with_clip(clip, c"Color/Tile", |tile| tile.set_color(color));
        ui::show_no_pattern(clip, *((swatch.context + EDIT_CONTEXT_CATEGORY_OFFSET) as *const u8));
    }
}

/// A job moving into the current panel, as a stock color row's on-confirm returns: its closure captures nothing and
/// passes on the window it's called with, the Paint screen (0x140f43e30 -> ... -> 0x140f43500(window)).
unsafe extern "system" fn item_confirm(_this: *const Closure<()>, out: *mut usize, window: usize) -> *mut usize {
    let mut enter = Function::new();
    unsafe { ui::function_job(out, enter.set(Closure { table: &ENTER_PANEL, captures: window })) };
    out
}

/// What that job runs (0x140f43d90, on the Paint screen): the confirm sound, the panel's cursor, the panel pushed on
/// the child windows.
unsafe extern "system" fn enter_panel(this: *const Closure<usize>) {
    #[repr(C)]
    struct Sound {
        id: u32,
        priority: u32,
        kind: u32,
        name: usize,
    }
    let screen = unsafe { (*this).captures };
    unsafe {
        let panel = *((screen + SCREEN_CURRENT_PANEL_OFFSET) as *const usize);
        if panel == 0 {
            return;
        }
        let play: unsafe extern "system" fn(*const Sound) = std::mem::transmute(0x140dc5440usize);
        let show_cursor: unsafe extern "system" fn(usize) = std::mem::transmute(0x140c76840usize);
        let push: unsafe extern "system" fn(usize, usize) = std::mem::transmute(0x140c6c060usize);
        play(&Sound { id: 100, priority: 2, kind: 1, name: 0x14390ba28 });
        show_cursor(panel + 0x4f0);
        // The child window stack of the panel state's first word (the screen itself, set up by 0x140f3fd90).
        push(*((screen + SCREEN_PANELS_OFFSET) as *const usize) + 0x10, panel);
    }
}

/// Appends the claimed rows to the Paint list's description, after WEATHERING.
unsafe fn append_rows(description: usize, context: usize) {
    let (start, end) = unsafe {
        (*((description + DESCRIPTION_ITEMS_OFFSET) as *const usize), *((description + DESCRIPTION_ITEMS_END_OFFSET) as *const usize))
    };
    let count = (end - start) / ITEM_SIZE;
    let last_kind = unsafe { *((end - ITEM_SIZE + ITEM_KIND_OFFSET) as *const i32) };
    if count != STOCK_ROW_COUNT || last_kind != WEATHERING_KIND {
        log(&format!("a list of {count} rows inside the row builder isn't the Paint list as built: no rows added"));
        return;
    }
    for row in claimed_rows() {
        let Some(settings) = settings(row) else { continue };
        let mut label = Label::new();
        let mut confirm = Function::new();
        unsafe {
            let item = ui::add_item(description, label.set(settings.label), confirm.set(Closure { table: &ITEM_CONFIRM, captures: () }));
            *((item + ITEM_KIND_OFFSET) as *mut i32) = KINDS[row];
            // The refresh's own reference on the context, released when the item's function is destroyed.
            ui::add_reference(context);
            ui::set_function(item + ui::ITEM_REFRESH_OFFSET, Closure { table: &ITEM_REFRESH, captures: Swatch { context, row } });
            // As the builder does to the rows that open a panel (0x140f41f20): without it the list doesn't grey out
            // around the row while its panel has the focus.
            *((item + ITEM_FLAGS_OFFSET) as *mut u8) |= ITEM_FLAG_PANEL;
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
    unsafe { *slot == device::SLOT || orig::<unsafe extern "system" fn(*const u8) -> bool>(&ORIG_IS_NORMAL_SLOT)(slot) }
}

type MakePanelFn = unsafe extern "system" fn(usize, *mut usize, u32) -> *mut usize;

// The game's switch has cases for the color slots' kinds only. A panel has a reference for the list, as the cases add.
unsafe extern "system" fn make_panel_impl(list: usize, out: *mut usize, kind: u32) -> *mut usize {
    let row = if kind == device::SLOT as u32 {
        None
    } else {
        match row_of_kind(kind) {
            Some(row) => Some(row),
            None => return unsafe { orig::<MakePanelFn>(&ORIG_MAKE_PANEL)(list, out, kind) },
        }
    };
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
