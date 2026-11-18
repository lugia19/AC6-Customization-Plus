//! The extra rows in the Paint screen's per-part list: slots 9-11, built by the game's own row builder and moved after
//! WEATHERING, each with a color and, optionally, a choice from a list (shown as the row's materials). A row is one
//! value for the whole AC, never per part. How the Paint screen builds its rows, panels and material menu, and what
//! each hook is for: docs/paint-rows.md. The rows' clips are in the Paint movie (tools/build_gfx.py).
//!
//! Three of the hooks also serve the DEVICE row's brightness (device.rs).
//!
//! Hooks:
//! - 0x140f418d0 row builder: marks when it's running, for the two below.
//! - 0x14045d300 slot list: inside the row builder, the claimed rows' slots are added.
//! - 0x14045de80 is-normal-slot: an extra slot with choices is, and DEVICE.
//! - 0x14045dc40 slot -> label: an extra slot's label is its row's.
//! - 0x140d3cb00 list -> list widget: the row builder's call moves the extra rows first.
//! - 0x140f40d10 kind -> edited slots: an extra row's kind edits its own slot.
//! - 0x14045a7f0 / 0x14045afc0 paint color reader / writer: an extra slot's color is in its row's storage.
//! - 0x140f3fd90 screen setup of the panels: ours is hidden like the game's three.
//! - 0x140f43610 row kind -> panel: an extra row with choices, and DEVICE's, get what the color slots' cases make,
//!   for their slot.
//! - 0x14045da60 first material of a slot: an extra slot's is its row's first choice.
//! - 0x14045a980 / 0x14045b070 paint material reader / writer: an extra slot's material is its row's choice, in its
//!   row's storage.
//! - 0x140f35c40 / 0x140e0cc30 material text (menu / panel): a choice's label; DEVICE's materials get their own.

use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU8, AtomicUsize, Ordering};

use crate::{Hook, device, log, orig, paint_data};

// The rows, top to bottom.
pub(crate) const ROW_THRUSTER: usize = 0;
pub(crate) const ROW_HUD: usize = 1;
const ROW_COUNT: usize = 3;
// Not 0-8: the real color slots and the color set / pattern / weathering row kinds.
static SLOTS: [u8; ROW_COUNT] = [9, 10, 11];
// The paint section a row keeps its values in, whichever part is selected: the color in color 0, the choice in
// material 0. No part uses these sections. Row 0 is on 7 because version 0.1.0 kept the thruster color there.
static SECTIONS: [u8; ROW_COUNT] = [7, 6, 9];
// A paint section: its six colors (u32s of RGBA bytes), its six materials (u16s). The game's writers take sections
// and a slot.
const SECTION_SIZE: usize = 0x44;
const SECTION_COLORS_OFFSET: usize = 8;
const SECTION_MATERIALS_OFFSET: usize = 0x20;
const STORAGE_SLOT: u8 = 0;
// A row with choices can also have a second color (its panel's third entry, panel.rs), in the section's color 1.
const SECOND_COLOR_STORAGE_SLOT: usize = 1;

/// A row's second color's name, if it has one.
pub(crate) fn second_color_label(row: usize) -> Option<&'static [u16]> {
    settings(row)?.second_color.map(|(label, _)| label)
}

fn second_color_offset(row: usize) -> usize {
    SECTIONS[row] as usize * SECTION_SIZE + SECTION_COLORS_OFFSET + SECOND_COLOR_STORAGE_SLOT * 4
}

/// The second color shown for a stored value: the stored one if it was picked, otherwise the row's default.
fn shown_second_color(row: usize, stored: u32) -> u32 {
    let default = settings(row).and_then(|s| s.second_color).map_or(ALPHA_MASK, |(_, default)| default);
    if stored & ALPHA_MASK == PICKED_ALPHA { stored | ALPHA_MASK } else { default }
}

/// A row's second color in a paint block (the edit context's working one), as stored (RGBA bytes, alpha 0xFF).
pub(crate) unsafe fn stored_second_color(paint: usize, row: usize) -> u32 {
    shown_second_color(row, unsafe { *((paint + second_color_offset(row)) as *const u32) })
}

/// A row's second color on the player's AC (0xRRGGBB). None while the player's data isn't loaded.
pub(crate) fn second_color(row: usize) -> Option<u32> {
    settings(row)?;
    Some(swap_red_blue(shown_second_color(row, paint_data::value(second_color_offset(row))?)))
}

/// Stores a row's second color in a paint block, marked as picked.
pub(crate) unsafe fn store_second_color(paint: usize, row: usize, color: u32) {
    unsafe { *((paint + second_color_offset(row)) as *mut u32) = (color & !ALPHA_MASK) | PICKED_ALPHA };
}

/// A claimed row. Texts are UTF-16 with a terminator; they and the rows are never freed (the game's labels keep the
/// texts' addresses).
#[derive(Clone, Copy)]
struct Row {
    label: &'static [u16],
    /// As stored (RGBA bytes), for a row the player hasn't picked a color in.
    default_color: u32,
    /// Empty for a color-only row.
    choices: &'static [&'static [u16]],
    default_choice: u32,
    /// The choices' name: the header of their entry in the panel, and the option list's title.
    choices_title: &'static [u16],
    /// A second color: its name and default (as stored).
    second_color: Option<(&'static [u16], u32)>,
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
// Makes a label from a text: (out, text, text ID) -> out. A label is 0x40 bytes and holds the text's address, not a
// copy. The game's text lookup (0x140cae6e0) ends in it, with the text it found for the ID.
const MAKE_LABEL_ADDR: usize = 0x140cb2970;
// Menu text category 200, no ID: the label only carries it along.
static LABEL_TEXT_ID: TextId = TextId { category: 200, id: 0 };
// The material in what the material menu's text function gets.
const MATERIAL_INFO_MATERIAL_OFFSET: usize = 0x20;

/// Alpha byte that marks a stored color as picked in its row. The game writes 0xFF into every color it stores (the
/// unused sections hold its default grey, older saves can have black with 0), so 0xFE only ever comes from us. It
/// travels with the color: saved with the AC and carried by designs.
const PICKED_ALPHA: u32 = 0xFE00_0000;
const ALPHA_MASK: u32 = 0xFF00_0000;
/// The same for a stored choice: 0xFE00 + the choice's index (the unused sections hold the default material, 18).
const PICKED_CHOICE: u16 = 0xFE00;
const CHOICE_MASK: u16 = 0x00FF;

static ORIG_SLOT_LIST: AtomicUsize = AtomicUsize::new(0);
static ORIG_IS_NORMAL_SLOT: AtomicUsize = AtomicUsize::new(0);
static ORIG_SLOT_LABEL: AtomicUsize = AtomicUsize::new(0);
static ORIG_BUILD_ROWS: AtomicUsize = AtomicUsize::new(0);
static ORIG_MAKE_LIST: AtomicUsize = AtomicUsize::new(0);
static ORIG_EDITED_SLOTS: AtomicUsize = AtomicUsize::new(0);
static ORIG_READ_COLOR: AtomicUsize = AtomicUsize::new(0);
static ORIG_WRITE_COLORS: AtomicUsize = AtomicUsize::new(0);
static ORIG_MAKE_PANEL: AtomicUsize = AtomicUsize::new(0);
static ORIG_MENU_MATERIAL_TEXT: AtomicUsize = AtomicUsize::new(0);
static ORIG_PANEL_MATERIAL_TEXT: AtomicUsize = AtomicUsize::new(0);
// What the panel switch's cases for the color slots do: build_panel(copy_panel_argument(temporary, list + 0x78),
// list + 8, slot), store the panel in the output, add a reference to it. The builder destroys the temporary.
const BUILD_PANEL_ADDR: usize = 0x140f49590;
const COPY_PANEL_ARGUMENT_ADDR: usize = 0x140c92e20;
const ADD_REFERENCE_ADDR: usize = 0x142923570;
const LIST_PANEL_ARGUMENT_OFFSET: usize = 0x78;
// The extra rows' own panel in the Paint movie (tools/build_gfx.py): the Color / Material panel with a color item
// in third place. At screen setup (0x140f3fd90) the game looks its three panels up by name in the screen's
// MainContent clip, with find_clip(parent handle, out handle, name), and hides them with set_visible(handle, false);
// the setup hook does the same for ours. A row's panel is built, and shown, when the cursor reaches the row
// (0x140f44b30 -> the panel switch), and ours is looked up again there: a handle is only a reference to the clip
// (Research\paint-popup\wide\report_clip_handles.md). list[0] is the Paint screen.
const ROWS_PANEL_NAME: &std::ffi::CStr = c"Color_Material_Color_Select";
const SCREEN_MAIN_CONTENT_OFFSET: usize = 0x1da0;
const FIND_CLIP_ADDR: usize = 0x140c928e0;
const SET_VISIBLE_ADDR: usize = 0x140c6aab0;
// A handle is destroyed with destroy_handle_value(handle + 0x28), then destroy_handle(handle).
const DESTROY_HANDLE_VALUE_ADDR: usize = 0x14160f3d0;
const DESTROY_HANDLE_ADDR: usize = 0x140c6a370;
const HANDLE_VALUE_OFFSET: usize = 0x28;
static ORIG_SET_UP_PANELS: AtomicUsize = AtomicUsize::new(0);
const LIST_PANEL_OWNER_OFFSET: usize = 8;
const OBJECT_REFERENCES_OFFSET: usize = 8;

// ---- the rows' settings ----------------------------------------------------------------------------------------------

fn leak_utf16(text: &str) -> &'static [u16] {
    text.encode_utf16().chain([0]).collect::<Vec<u16>>().leak()
}

fn settings(row: usize) -> Option<&'static Row> {
    unsafe { ROWS[row].load(Ordering::Relaxed).as_ref() }
}

fn set_settings(row: usize, settings: Row) {
    ROWS[row].store(Box::leak(Box::new(settings)), Ordering::Relaxed);
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

/// The claimed row with this slot, if it has choices.
fn choices_row_of(slot: u8) -> Option<(usize, &'static Row)> {
    row_of(slot).filter(|(_, settings)| !settings.choices.is_empty())
}

/// 0xRRGGBB <-> the game's stored color (RGBA bytes; alpha left 0 / dropped).
fn swap_red_blue(color: u32) -> u32 {
    let [b, g, r, _] = color.to_le_bytes();
    u32::from_le_bytes([r, g, b, 0])
}

/// Shows a row in the Paint list, with a label, showing `default_color` (0xRRGGBB) until the player picks a color.
pub(crate) fn claim(row: usize, label: &str, default_color: u32) {
    let default_color = swap_red_blue(default_color) | ALPHA_MASK;
    set_settings(row, Row { label: leak_utf16(label), default_color, choices: &[], default_choice: 0, choices_title: &[0], second_color: None });
}

/// Gives a claimed row a list of choices next to its color: confirming the row then opens the game's Color /
/// Material panel, whose Material menu lists them. `default_choice` is the one a row starts on. The regulation must
/// have a material per choice (see MAX_CHOICES).
pub(crate) fn set_choices(row: usize, title: &str, labels: &[&str], default_choice: u32) {
    let Some(&old) = settings(row) else { return };
    assert!(!labels.is_empty() && labels.len() <= u8::MAX as usize && (default_choice as usize) < labels.len());
    let choices = labels.iter().map(|label| leak_utf16(label)).collect::<Vec<_>>().leak();
    set_settings(row, Row { choices, default_choice, choices_title: leak_utf16(title), ..old });
}

/// Gives a row with choices a second color, `label` in its panel, showing `default_color` (0xRRGGBB) until picked.
pub(crate) fn set_second_color(row: usize, label: &str, default_color: u32) {
    let Some(&old) = settings(row) else { return };
    let default_color = swap_red_blue(default_color) | ALPHA_MASK;
    set_settings(row, Row { second_color: Some((leak_utf16(label), default_color)), ..old });
}

// What the panel's own entries need (panel.rs).

/// The row with choices whose slot this is.
pub(crate) fn row_with_choices(slot: u8) -> Option<usize> {
    choices_row_of(slot).map(|(row, _)| row)
}

/// A row's choices' name and labels.
pub(crate) fn choices_of(row: usize) -> (&'static [u16], &'static [&'static [u16]]) {
    settings(row).map(|s| (s.choices_title, s.choices)).unwrap_or((&[0], &[]))
}

/// A row's choice in a paint block (the edit context's working one), as an index.
pub(crate) unsafe fn stored_choice(paint: usize, row: usize) -> u32 {
    let Some(settings) = settings(row) else { return 0 };
    shown_choice(settings, unsafe { *((paint + choice_offset(row)) as *const u16) })
}

/// Stores a row's choice in a paint block, marked as picked.
pub(crate) unsafe fn store_choice(paint: usize, row: usize, index: u32) {
    unsafe { *((paint + choice_offset(row)) as *mut u16) = PICKED_CHOICE | index as u16 };
}

fn color_offset(row: usize) -> usize {
    SECTIONS[row] as usize * SECTION_SIZE + SECTION_COLORS_OFFSET + STORAGE_SLOT as usize * 4
}

fn choice_offset(row: usize) -> usize {
    SECTIONS[row] as usize * SECTION_SIZE + SECTION_MATERIALS_OFFSET + STORAGE_SLOT as usize * 2
}

/// The row's color for a stored value, opaque: the stored color if it was picked in the row, otherwise the default.
fn shown_color(settings: &Row, stored: u32) -> u32 {
    let picked = stored & ALPHA_MASK == PICKED_ALPHA;
    if picked { stored | ALPHA_MASK } else { settings.default_color }
}

/// The row's choice for a stored value: the stored one if it was picked in the row and still exists, otherwise the
/// default.
fn shown_choice(settings: &Row, stored: u16) -> u32 {
    let picked = stored & !CHOICE_MASK == PICKED_CHOICE;
    let index = (stored & CHOICE_MASK) as u32;
    if picked && (index as usize) < settings.choices.len() { index } else { settings.default_choice }
}

/// A row's color on the player's AC (0xRRGGBB): the picked one, or the default. None while the player's data isn't
/// loaded.
pub(crate) fn color(row: usize) -> Option<u32> {
    Some(swap_red_blue(shown_color(settings(row)?, paint_data::value(color_offset(row))?)))
}

/// A row's choice on the player's AC, as an index into its labels: the picked one, or the default. None if it has no
/// choices or the player's data isn't loaded.
pub(crate) fn choice(row: usize) -> Option<u32> {
    let settings = settings(row).filter(|settings| !settings.choices.is_empty())?;
    Some(shown_choice(settings, paint_data::value(choice_offset(row))?))
}

// ---- game functions --------------------------------------------------------------------------------------------------

type SlotRange = [*const u8; 2];

fn range(slot: &'static u8) -> SlotRange {
    let r = std::slice::from_ref(slot).as_ptr_range();
    [r.start, r.end]
}

#[repr(C, align(16))]
pub(crate) struct Label([usize; 8]);

impl Label {
    pub(crate) fn new() -> Self {
        Label([0; 8])
    }
}

#[repr(C)]
struct TextId {
    category: i16,
    id: i32,
}

/// Constructs a label with our text in `out`.
pub(crate) unsafe fn make_label(out: *mut Label, text: &'static [u16]) -> *mut Label {
    type MakeLabelFn = unsafe extern "system" fn(*mut Label, *const u16, *const TextId) -> *mut Label;
    let make: MakeLabelFn = unsafe { std::mem::transmute(MAKE_LABEL_ADDR) };
    unsafe { make(out, text.as_ptr(), &LABEL_TEXT_ID) }
}

/// Room for a row (0x308 bytes; the game's own caller of GET_ROW_ADDR passes 0x310).
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

// Only inside the row builder (nothing else asks for the list there): other code fills six-color arrays from this
// list.
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
        Some((_, settings)) => !settings.choices.is_empty(),
        None => slot_number == device::SLOT || unsafe { orig::<unsafe extern "system" fn(*const u8) -> bool>(&ORIG_IS_NORMAL_SLOT)(slot) },
    }
}

unsafe extern "system" fn slot_label_impl(slot: *const u8, out: *mut Label) -> *mut Label {
    let Some((_, settings)) = row_of(unsafe { *slot }) else {
        type SlotLabelFn = unsafe extern "system" fn(*const u8, *mut Label) -> *mut Label;
        return unsafe { orig::<SlotLabelFn>(&ORIG_SLOT_LABEL)(slot, out) };
    };
    unsafe { make_label(out, settings.label) }
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
    let stored = unsafe { *((paint + color_offset(row)) as *const u32) };
    unsafe { *out = shown_color(settings, stored) };
    out
}

type WriteColorsFn = unsafe extern "system" fn(usize, *const SlotRange, u8, u32);

unsafe extern "system" fn write_colors_impl(paint: usize, parts: *const SlotRange, slot: u8, color: u32) {
    if !SLOTS.contains(&slot) {
        return unsafe { orig::<WriteColorsFn>(&ORIG_WRITE_COLORS)(paint, parts, slot, color) };
    }
    // To the row's section, marked as picked.
    if let Some((row, _)) = row_of(slot) {
        let picked = (color & !ALPHA_MASK) | PICKED_ALPHA;
        unsafe { orig::<WriteColorsFn>(&ORIG_WRITE_COLORS)(paint, &range(&SECTIONS[row]), STORAGE_SLOT, picked) }
    }
}

// ---- choices ---------------------------------------------------------------------------------------------------------

/// Room for the panel builder's first argument (0x60 bytes).
#[repr(C, align(16))]
struct PanelArgument([u8; 0x80]);

type MakePanelFn = unsafe extern "system" fn(usize, *mut usize, u32) -> *mut usize;

unsafe extern "system" fn make_panel_impl(list: usize, out: *mut usize, kind: u32) -> *mut usize {
    let slot = if kind == device::SLOT as u32 {
        Some(device::SLOT)
    } else {
        row_of_kind(kind as i64).filter(|(_, settings)| !settings.choices.is_empty()).map(|(row, _)| SLOTS[row])
    };
    let Some(slot) = slot else {
        return unsafe { orig::<MakePanelFn>(&ORIG_MAKE_PANEL)(list, out, kind) };
    };
    // The switch has no case for the kind: do what its cases for the color slots do, with this slot.
    type CopyPanelArgumentFn = unsafe extern "system" fn(*mut PanelArgument, usize) -> *mut PanelArgument;
    let copy_panel_argument: CopyPanelArgumentFn = unsafe { std::mem::transmute(COPY_PANEL_ARGUMENT_ADDR) };
    type BuildPanelFn = unsafe extern "system" fn(*mut PanelArgument, usize, u8) -> usize;
    let build_panel: BuildPanelFn = unsafe { std::mem::transmute(BUILD_PANEL_ADDR) };
    let add_reference: unsafe extern "system" fn(usize) = unsafe { std::mem::transmute(ADD_REFERENCE_ADDR) };
    let mut argument = PanelArgument([0; 0x80]);
    crate::panel::building_panel();
    unsafe {
        let argument = if slot == device::SLOT {
            copy_panel_argument(&mut argument, list + LIST_PANEL_ARGUMENT_OFFSET)
        } else {
            find_rows_panel(*(list as *const usize), &mut argument)
        };
        let panel = build_panel(argument, list + LIST_PANEL_OWNER_OFFSET, slot);
        *out = panel;
        if panel != 0 {
            add_reference(panel + OBJECT_REFERENCES_OFFSET);
        }
    }
    out
}

type FindClipFn = unsafe extern "system" fn(usize, *mut PanelArgument, *const std::ffi::c_char) -> *mut PanelArgument;

/// Looks the extra rows' panel up in the Paint screen's movie, into `out` (to be destroyed by whoever gets it).
unsafe fn find_rows_panel(screen: usize, out: *mut PanelArgument) -> *mut PanelArgument {
    let find_clip: FindClipFn = unsafe { std::mem::transmute(FIND_CLIP_ADDR) };
    unsafe { find_clip(screen + SCREEN_MAIN_CONTENT_OFFSET, out, ROWS_PANEL_NAME.as_ptr()) }
}

type SetUpPanelsFn = unsafe extern "system" fn(usize, usize, usize, usize) -> usize;

unsafe extern "system" fn set_up_panels_impl(list: usize, content: usize, screen: usize, a4: usize) -> usize {
    let result = unsafe { orig::<SetUpPanelsFn>(&ORIG_SET_UP_PANELS)(list, content, screen, a4) };
    let set_visible: unsafe extern "system" fn(*mut PanelArgument, u8) = unsafe { std::mem::transmute(SET_VISIBLE_ADDR) };
    let destroy_value: unsafe extern "system" fn(usize) = unsafe { std::mem::transmute(DESTROY_HANDLE_VALUE_ADDR) };
    let destroy: unsafe extern "system" fn(*mut PanelArgument) = unsafe { std::mem::transmute(DESTROY_HANDLE_ADDR) };
    let mut handle = PanelArgument([0; 0x80]);
    unsafe {
        let panel = find_rows_panel(screen, &mut handle);
        set_visible(panel, 0);
        destroy_value(panel as usize + HANDLE_VALUE_OFFSET);
        destroy(panel);
    }
    result
}

unsafe extern "system" fn menu_material_text_impl(info: usize, out: *mut Label) -> *mut Label {
    let material = unsafe { *((info + MATERIAL_INFO_MATERIAL_OFFSET) as *const u32) };
    let Some(text) = device::material_text(material) else {
        type MenuMaterialTextFn = unsafe extern "system" fn(usize, *mut Label) -> *mut Label;
        return unsafe { orig::<MenuMaterialTextFn>(&ORIG_MENU_MATERIAL_TEXT)(info, out) };
    };
    unsafe { make_label(out, text) }
}

unsafe extern "system" fn panel_material_text_impl(out: *mut Label, material: u32) -> *mut Label {
    let Some(text) = device::material_text(material) else {
        type PanelMaterialTextFn = unsafe extern "system" fn(*mut Label, u32) -> *mut Label;
        return unsafe { orig::<PanelMaterialTextFn>(&ORIG_PANEL_MATERIAL_TEXT)(out, material) };
    };
    unsafe { make_label(out, text) }
}

/// The hooks, for lib.rs's hook table.
pub(crate) fn hooks() -> [Hook; 12] {
    [
        // mov [rsp+10],rdx / mov [rsp+8],rcx
        (0x140f3fd90, [0x48, 0x89, 0x54, 0x24, 0x10, 0x48], set_up_panels_impl as *const (), &ORIG_SET_UP_PANELS),
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
        // mov rax,rsp / push rdi / sub rsp,..
        (0x140f35c40, [0x48, 0x8b, 0xc4, 0x57, 0x48, 0x81], menu_material_text_impl as *const (), &ORIG_MENU_MATERIAL_TEXT),
        // push rbp / push rsi / push rdi / sub rsp,..
        (0x140e0cc30, [0x40, 0x55, 0x56, 0x57, 0x48, 0x81], panel_material_text_impl as *const (), &ORIG_PANEL_MATERIAL_TEXT),
    ]
}
