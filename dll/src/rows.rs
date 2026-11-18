//! The extra rows in the Paint screen's per-part list: slots 9-11, built by the game's own row builder and moved after
//! WEATHERING. A row is one value set for the whole AC, never per part: a color, and optionally a choice from a list
//! and a second color, which its panel (panel.rs) offers. How the Paint screen builds its rows and panels, and what
//! each hook is for: docs/paint-rows.md. The rows' clips are in the Paint movie (tools/build_gfx.py).
//!
//! Hooks:
//! - 0x140f418d0 row builder: marks when it's running, for the two below.
//! - 0x14045d300 slot list: inside the row builder, the claimed rows' slots are added.
//! - 0x14045de80 is-normal-slot: a row with choices is (it gets a panel), and DEVICE (its brightness, device.rs).
//! - 0x14045dc40 slot -> label: a row's label.
//! - 0x140d3cb00 list -> list widget: the row builder's call moves the extra rows first.
//! - 0x140f40d10 kind -> edited slots: a row's kind edits its own slot.
//! - 0x14045a7f0 / 0x14045afc0 paint color reader / writer: a row's color is in its row's paint section (the row's
//!   swatch in the list reads it every frame).
//! - 0x140f43610 row kind -> panel: our rows with choices and DEVICE get the panel panel.rs builds.

use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU8, AtomicUsize, Ordering};

use crate::ui::{self, Label, leak_utf16};
use crate::{Hook, device, log, orig, paint_data, panel};

// The rows, top to bottom (row 2 is free).
pub(crate) const ROW_THRUSTER: usize = 0;
pub(crate) const ROW_HUD: usize = 1;
const ROW_COUNT: usize = 3;
// Not 0-8: the real color slots and the color set / pattern / weathering row kinds.
static SLOTS: [u8; ROW_COUNT] = [9, 10, 11];
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

// What the row builder loops over: the six color slots, then the claimed rows' slots.
static BUILDER_SLOTS: [AtomicU8; 6 + ROW_COUNT] = [const { AtomicU8::new(0) }; 6 + ROW_COUNT];
// Set while the row builder runs (menu code runs on one thread).
static IN_ROW_BUILDER: AtomicBool = AtomicBool::new(false);
// The list's rows: a vector of 0x308-byte rows. Before the extra ones: COLOR SET and the six colors; after them:
// PATTERN and WEATHERING.
const LIST_ROWS_OFFSET: usize = 0xc8;
const LIST_ROWS_END_OFFSET: usize = 0xd0;
const ROW_SIZE: usize = 0x308;
const ROW_KIND_OFFSET: usize = 0x98;
const ROWS_BEFORE: usize = 7;
const ROWS_AFTER: usize = 2;
// A row's copy constructor (to, from) and destructor.
const COPY_ROW_ADDR: usize = 0x140c81610;
const DESTROY_ROW_ADDR: usize = 0x140c82e00;
// Copy-constructs the list's current row into a buffer and returns it.
const GET_ROW_ADDR: usize = 0x140d40830;

static ORIG_SLOT_LIST: AtomicUsize = AtomicUsize::new(0);
static ORIG_IS_NORMAL_SLOT: AtomicUsize = AtomicUsize::new(0);
static ORIG_SLOT_LABEL: AtomicUsize = AtomicUsize::new(0);
static ORIG_BUILD_ROWS: AtomicUsize = AtomicUsize::new(0);
static ORIG_MAKE_LIST: AtomicUsize = AtomicUsize::new(0);
static ORIG_EDITED_SLOTS: AtomicUsize = AtomicUsize::new(0);
static ORIG_READ_COLOR: AtomicUsize = AtomicUsize::new(0);
static ORIG_WRITE_COLORS: AtomicUsize = AtomicUsize::new(0);
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

/// The claimed row with this slot.
fn row_of(slot: u8) -> Option<(usize, &'static Row)> {
    let row = SLOTS.iter().position(|&s| s == slot)?;
    Some((row, settings(row)?))
}

/// The claimed row whose rows have this kind (a row's kind is its slot).
fn row_of_kind(kind: i64) -> Option<(usize, &'static Row)> {
    row_of(u8::try_from(kind).ok()?)
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

/// Gives a claimed row a list of choices, under `title`: the row then has a panel, whose choice entry lists them.
/// `default` is the one a row starts on.
pub(crate) fn set_choices(row: usize, title: &str, labels: &[&str], default: u32) {
    assert!(!labels.is_empty() && labels.len() <= CHOICE_MASK as usize && (default as usize) < labels.len());
    let labels = labels.iter().map(|label| leak_utf16(label)).collect::<Vec<_>>().leak();
    update(row, |r| r.choices = Some(Choices { title: leak_utf16(title), labels, default }));
}

/// Gives a row with choices a second color in its panel, `label`, showing `default_color` (0xRRGGBB) until picked.
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

/// A row's choices: their title and labels.
pub(crate) fn choices(row: usize) -> (&'static [u16], Vec<&'static [u16]>) {
    match settings(row).and_then(|s| s.choices) {
        Some(choices) => (choices.title, choices.labels.to_vec()),
        None => (&[0], Vec::new()),
    }
}

/// A row's choice, as an index.
pub(crate) unsafe fn stored_choice(paint: usize, row: usize) -> u32 {
    let Some(choices) = settings(row).and_then(|s| s.choices) else { return 0 };
    shown_choice(&choices, unsafe { *((paint + choice_offset(row)) as *const u16) })
}

pub(crate) unsafe fn store_choice(paint: usize, row: usize, index: u32) {
    unsafe { *((paint + choice_offset(row)) as *mut u16) = PICKED_CHOICE | index as u16 };
}

// ---- game functions --------------------------------------------------------------------------------------------------

type SlotRange = [*const u8; 2];

fn range(slot: &'static u8) -> SlotRange {
    let r = std::slice::from_ref(slot).as_ptr_range();
    [r.start, r.end]
}

/// Room for a list row (0x308 bytes; the game's own caller of GET_ROW_ADDR passes 0x310).
#[repr(C, align(16))]
struct RowBuffer([u8; 0x310]);

impl RowBuffer {
    fn new() -> Self {
        RowBuffer([0; 0x310])
    }

    fn address(&mut self) -> usize {
        self as *mut Self as usize
    }
}

unsafe fn copy_row(to: usize, from: usize) {
    let copy: unsafe extern "system" fn(usize, usize) -> usize = unsafe { std::mem::transmute(COPY_ROW_ADDR) };
    unsafe { copy(to, from) };
}

unsafe fn destroy_row(row: usize) {
    let destroy: unsafe extern "system" fn(usize) = unsafe { std::mem::transmute(DESTROY_ROW_ADDR) };
    unsafe { destroy(row) };
}

// ---- rows ------------------------------------------------------------------------------------------------------------

type BuildRowsFn = unsafe extern "system" fn(usize, usize, usize, usize) -> usize;

unsafe extern "system" fn build_rows_impl(out: usize, screen: usize, a3: usize, a4: usize) -> usize {
    IN_ROW_BUILDER.store(true, Ordering::Relaxed);
    let result = unsafe { orig::<BuildRowsFn>(&ORIG_BUILD_ROWS)(out, screen, a3, a4) };
    IN_ROW_BUILDER.store(false, Ordering::Relaxed);
    result
}

// Only inside the row builder: other code fills six-entry arrays from this list (0x140e0c9b0 gathers the category's
// colors into them), which slots 9-11 would overrun.
unsafe extern "system" fn slot_list_impl(out: *mut SlotRange) -> *mut SlotRange {
    let result = unsafe { orig::<unsafe extern "system" fn(*mut SlotRange) -> *mut SlotRange>(&ORIG_SLOT_LIST)(out) };
    if IN_ROW_BUILDER.load(Ordering::Relaxed) {
        let mut count = 0;
        for slot in (0..6).chain(claimed_rows().map(|row| SLOTS[row])) {
            BUILDER_SLOTS[count].store(slot, Ordering::Relaxed);
            count += 1;
        }
        let start = BUILDER_SLOTS.as_ptr() as *const u8;
        unsafe { *out = [start, start.add(count)] };
    }
    result
}

unsafe extern "system" fn is_normal_slot_impl(slot: *const u8) -> bool {
    let slot_number = unsafe { *slot };
    match row_of(slot_number) {
        Some((_, settings)) => settings.choices.is_some(),
        None => slot_number == device::SLOT || unsafe { orig::<unsafe extern "system" fn(*const u8) -> bool>(&ORIG_IS_NORMAL_SLOT)(slot) },
    }
}

unsafe extern "system" fn slot_label_impl(slot: *const u8, out: *mut Label) -> *mut Label {
    let Some((_, settings)) = row_of(unsafe { *slot }) else {
        type SlotLabelFn = unsafe extern "system" fn(*const u8, *mut Label) -> *mut Label;
        return unsafe { orig::<SlotLabelFn>(&ORIG_SLOT_LABEL)(slot, out) };
    };
    unsafe { (*out).set(settings.label) }
}

type MakeListFn = unsafe extern "system" fn(usize, usize, usize, usize) -> usize;

// The row builder hands its finished list to this, which copies it into the list widget.
unsafe extern "system" fn make_list_impl(list: usize, out: usize, a3: usize, a4: usize) -> usize {
    if IN_ROW_BUILDER.load(Ordering::Relaxed) {
        unsafe { move_rows(list) };
    }
    unsafe { orig::<MakeListFn>(&ORIG_MAKE_LIST)(list, out, a3, a4) }
}

/// The builder's loop added the extra rows right after the six colors: moves them after WEATHERING, where their
/// clips are. A row is moved the way the row vector does it when it grows: copy-construct, destroy the old one.
unsafe fn move_rows(list: usize) {
    let extra = claimed_rows().count();
    if extra == 0 {
        return;
    }
    let (start, end) = unsafe { (*((list + LIST_ROWS_OFFSET) as *const usize), *((list + LIST_ROWS_END_OFFSET) as *const usize)) };
    let count = (end - start) / ROW_SIZE;
    let row = |index: usize| start + index * ROW_SIZE;
    // Only the list we expect: the extra rows' kinds (their slots) where the loop put them.
    let kind = |index: usize| unsafe { *((row(index) + ROW_KIND_OFFSET) as *const i32) };
    let as_built = count == ROWS_BEFORE + extra + ROWS_AFTER
        && claimed_rows().enumerate().all(|(i, claimed)| kind(ROWS_BEFORE + i) == SLOTS[claimed] as i32);
    if !as_built {
        log(&format!("a list of {count} rows inside the row builder isn't the Paint list as built: rows not moved"));
        return;
    }
    let mut moved: Vec<RowBuffer> = (0..extra).map(|_| RowBuffer::new()).collect();
    unsafe {
        for (i, buffer) in moved.iter_mut().enumerate() {
            copy_row(buffer.address(), row(ROWS_BEFORE + i));
            destroy_row(row(ROWS_BEFORE + i));
        }
        for i in 0..ROWS_AFTER {
            copy_row(row(ROWS_BEFORE + i), row(ROWS_BEFORE + extra + i));
            destroy_row(row(ROWS_BEFORE + extra + i));
        }
        for (i, buffer) in moved.iter_mut().enumerate() {
            copy_row(row(ROWS_BEFORE + ROWS_AFTER + i), buffer.address());
            destroy_row(buffer.address());
        }
    }
}

/// The kind of the list's current row, read the way 0x140f40d10 does it.
fn current_row_kind(list: usize) -> i32 {
    let get_row: unsafe extern "system" fn(usize, usize) -> usize = unsafe { std::mem::transmute(GET_ROW_ADDR) };
    let mut copy = RowBuffer::new();
    unsafe {
        let row = get_row(list, copy.address());
        let kind = *((row + ROW_KIND_OFFSET) as *const i32);
        destroy_row(copy.address());
        kind
    }
}

unsafe extern "system" fn edited_slots_impl(list: *const usize, out: *mut SlotRange) -> *mut SlotRange {
    type EditedSlotsFn = unsafe extern "system" fn(*const usize, *mut SlotRange) -> *mut SlotRange;
    let result = unsafe { orig::<EditedSlotsFn>(&ORIG_EDITED_SLOTS)(list, out) };
    // Unknown kinds (pattern, weathering, ours) get an empty range; only then check whether it's ours.
    let [start, end] = unsafe { *out };
    if start == end {
        if let Some((row, _)) = row_of_kind(current_row_kind(unsafe { *list }) as i64) {
            unsafe { *out = range(&SLOTS[row]) };
        }
    }
    result
}

// ---- color -----------------------------------------------------------------------------------------------------------

// Called every frame for each swatch.
unsafe extern "system" fn read_color_impl(paint: usize, out: *mut u32, parts: *const SlotRange, slot: u8) -> *mut u32 {
    let Some((row, settings)) = row_of(slot) else {
        type ReadColorFn = unsafe extern "system" fn(usize, *mut u32, *const SlotRange, u8) -> *mut u32;
        return unsafe { orig::<ReadColorFn>(&ORIG_READ_COLOR)(paint, out, parts, slot) };
    };
    // Six colors per section (slot 9's address is the pattern bytes): a row's color is in the row's section instead
    // of the selected part's.
    let stored = unsafe { *((paint + color_offset(row, 0)) as *const u32) };
    unsafe { *out = shown_color(settings, 0, stored) };
    out
}

type WriteColorsFn = unsafe extern "system" fn(usize, *const SlotRange, u8, u32);

unsafe extern "system" fn write_colors_impl(paint: usize, parts: *const SlotRange, slot: u8, color: u32) {
    if !SLOTS.contains(&slot) {
        return unsafe { orig::<WriteColorsFn>(&ORIG_WRITE_COLORS)(paint, parts, slot, color) };
    }
    // To the row's section, marked as picked.
    if let Some((row, _)) = row_of(slot) {
        unsafe { store_color(paint, row, 0, color) };
    }
}

// ---- panels ----------------------------------------------------------------------------------------------------------

type MakePanelFn = unsafe extern "system" fn(usize, *mut usize, u32) -> *mut usize;

// The game's switch has cases for the color slots' kinds only. A panel has a reference for the list, as the cases add.
unsafe extern "system" fn make_panel_impl(list: usize, out: *mut usize, kind: u32) -> *mut usize {
    let row = if kind == device::SLOT as u32 {
        None
    } else {
        match row_of_kind(kind as i64) {
            Some((row, settings)) if settings.choices.is_some() => Some(row),
            _ => return unsafe { orig::<MakePanelFn>(&ORIG_MAKE_PANEL)(list, out, kind) },
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
pub(crate) fn hooks() -> [Hook; 9] {
    [
        // jmp (the function starts elsewhere)
        (0x140f418d0, [0xe9, 0x83, 0x90, 0x55, 0x05, 0x48], build_rows_impl as *const (), &ORIG_BUILD_ROWS),
        // push rbx / sub rsp,30
        (0x14045d300, [0x40, 0x53, 0x48, 0x83, 0xec, 0x30], slot_list_impl as *const (), &ORIG_SLOT_LIST),
        // movzx eax,byte [rcx] / cmp al,5
        (0x14045de80, [0x0f, 0xb6, 0x01, 0x3c, 0x05, 0x74], is_normal_slot_impl as *const (), &ORIG_IS_NORMAL_SLOT),
        // mov rax,rsp / push rbp / push rdi / push r14
        (0x14045dc40, [0x48, 0x8b, 0xc4, 0x55, 0x57, 0x41], slot_label_impl as *const (), &ORIG_SLOT_LABEL),
        // push rbp / push rbx / push rsi / push rdi / push r14
        (0x140d3cb00, [0x40, 0x55, 0x53, 0x56, 0x57, 0x41], make_list_impl as *const (), &ORIG_MAKE_LIST),
        // push rdi / sub rsp,350
        (0x140f40d10, [0x40, 0x57, 0x48, 0x81, 0xec, 0x50], edited_slots_impl as *const (), &ORIG_EDITED_SLOTS),
        // jmp (the function starts elsewhere)
        (0x14045a7f0, [0xe9, 0x29, 0x87, 0x59, 0x05, 0x66], read_color_impl as *const (), &ORIG_READ_COLOR),
        // mov [rsp+8],rbx
        (0x14045afc0, [0x48, 0x89, 0x5c, 0x24, 0x08, 0x48], write_colors_impl as *const (), &ORIG_WRITE_COLORS),
        // mov rax,rsp / mov [rax+10],rdx
        (0x140f43610, [0x48, 0x8b, 0xc4, 0x48, 0x89, 0x50], make_panel_impl as *const (), &ORIG_MAKE_PANEL),
    ]
}
