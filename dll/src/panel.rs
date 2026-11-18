//! The panel of a row with choices: Color (the game's own entry), the choices (the builder's Material entry, taken
//! over) and a second color (an entry of our own), on the rows' own panel clip (rows.rs).
//! Research: AC6_tools\Research\paint-popup (OVERVIEW.md; wide2\report_color_picker.md for the picker recipe).
//!
//! A panel is a list description filled with items; an item is a label, an on-confirm std::function (returns the
//! job that opens what comes next) and a refresh std::function (fills the item's clip every frame). The Color /
//! Material builder adds Color with 0x140f3b1c0(screen, description, slot, label), then Material, then asks
//! 0x140e0c8e0 whether to add Paint Pattern's Apply: there, ours is appended.
//! - refresh: the swatch ("Color/Tile", or "Color") and the "R: G: B:" text ("Value_0") from the stored color;
//! - on confirm: the game's color picker (0x140f37450) with our title, the stored color and our callback, which the
//!   picker calls on every change (live preview); it writes into the edit context's working paint block, which the
//!   picker restores on cancel.
//! Both values are kept in the row's paint section (rows.rs), written into the working paint block.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use crate::{Hook, log, orig, rows};

static ORIG_ADD_COLOR_ENTRY: AtomicUsize = AtomicUsize::new(0);
static ORIG_PATTERN_CHECK: AtomicUsize = AtomicUsize::new(0);
// The panel being built for one of our rows or DEVICE: the Paint screen, the list description, the row's slot.
static PENDING: [AtomicUsize; 3] = [const { AtomicUsize::new(0) }; 3];
// Set by the panel switch (rows.rs) right before it builds such a panel: the Color entry function is also used for
// color-only rows in the main list, which must not count.
static BUILDING: AtomicBool = AtomicBool::new(false);

/// The panel switch is about to build the panel of one of our rows or DEVICE.
pub(crate) fn building_panel() {
    BUILDING.store(true, Ordering::Relaxed);
}

type AddColorEntryFn = unsafe extern "system" fn(usize, usize, u8, usize) -> u64;
type PatternCheckFn = unsafe extern "system" fn(usize, usize, u8) -> u64;

// Game functions.
const MAKE_ROW_ADDR: usize = 0x140d92d80; // (row out 0x88, label) -> row
const DESTROY_ROW_ADDR: usize = 0x140c82fd0;
const APPEND_ITEM_ADDR: usize = 0x140d3c600; // (description, row, on-confirm std::function (consumed), value) -> description
const SWAP_FUNCTION_ADDR: usize = 0x140f3e380; // (std::function, std::function)
const OPEN_PICKER_ADDR: usize = 0x140f37450; // (screen, job** out, a3, title label, color, on-color std::function)
const MOVE_JOB_ADDR: usize = 0x140c81f20; // (job** out, job** from)
const FIND_CLIP_ADDR: usize = 0x140c928e0; // (parent handle, out handle, name) -> out
const IS_VALID_CLIP_ADDR: usize = 0x140c6a500;
const DESTROY_HANDLE_VALUE_ADDR: usize = 0x14160f3d0;
const DESTROY_HANDLE_ADDR: usize = 0x140c6a370;
const SET_TEXT_ADDR: usize = 0x140c8cba0; // (handle + 8, UTF-16 text)
const SET_COLOR_ADDR: usize = 0x141614a80; // (handle + 0x28, color, channel range)
const SET_VISIBLE_ADDR: usize = 0x140c6aab0; // (handle, visible)

const DESCRIPTION_ITEMS_END_OFFSET: usize = 0xd0;
const ITEM_SIZE: usize = 0x308;
const ITEM_REFRESH_OFFSET: usize = 0x1b8;
const SCREEN_EDIT_CONTEXT_OFFSET: usize = 0x440;
const EDIT_CONTEXT_PAINT_OFFSET: usize = 0x40;

pub(crate) fn hooks() -> [Hook; 2] {
    [
        // mov [rsp+18],r8b / push rbp
        (0x140f3b1c0, [0x44, 0x88, 0x44, 0x24, 0x18, 0x55], add_color_entry_impl as *const (), &ORIG_ADD_COLOR_ENTRY),
        // mov [rsp+8],rbx / push rdi
        (0x140e0c8e0, [0x48, 0x89, 0x5c, 0x24, 0x08, 0x57], pattern_check_impl as *const (), &ORIG_PATTERN_CHECK),
    ]
}

// ---- std::function (MSVC: 0x40 bytes, the implementation inline at +0, a pointer to it at +0x38) ------------------

/// Our std::function implementations: a table pointer and what they capture.
#[repr(C)]
#[derive(Clone, Copy)]
struct Closure {
    table: &'static FunctionTable,
    screen: usize,
    row: usize,
    index: usize, // the option (option list)
}

/// The implementation's virtual table: copy, move, call, target type, delete, get.
#[repr(C)]
struct FunctionTable([*const (); 6]);

// Read-only after construction.
unsafe impl Sync for FunctionTable {}

#[repr(C, align(16))]
struct Function {
    storage: [usize; 7],
    implementation: *mut Closure,
}

impl Function {
    /// An empty slot, then filled in place (the pointer at +0x38 points into the object itself).
    fn new() -> Self {
        Function { storage: [0; 7], implementation: std::ptr::null_mut() }
    }

    fn set(&mut self, closure: Closure) {
        let at = self.storage.as_mut_ptr() as *mut Closure;
        unsafe { at.write(closure) };
        self.implementation = at;
    }
}

unsafe extern "system" fn closure_copy(this: *const Closure, to: *mut Closure) -> *mut Closure {
    unsafe { to.write(*this) };
    to
}

unsafe extern "system" fn closure_target_type(_this: *const Closure) -> usize {
    static TYPE: u8 = 0;
    &TYPE as *const u8 as usize
}

unsafe extern "system" fn closure_delete(_this: *mut Closure, _deallocate: u8) {}

unsafe extern "system" fn closure_get(this: *mut Closure) -> *mut usize {
    unsafe { (this as *mut usize).add(1) }
}

static REFRESH_TABLE: FunctionTable = FunctionTable([
    closure_copy as *const (), closure_copy as *const (), refresh_call as *const (),
    closure_target_type as *const (), closure_delete as *const (), closure_get as *const (),
]);
static CONFIRM_TABLE: FunctionTable = FunctionTable([
    closure_copy as *const (), closure_copy as *const (), confirm_call as *const (),
    closure_target_type as *const (), closure_delete as *const (), closure_get as *const (),
]);
static CHOICE_REFRESH_TABLE: FunctionTable = FunctionTable([
    closure_copy as *const (), closure_copy as *const (), choice_refresh_call as *const (),
    closure_target_type as *const (), closure_delete as *const (), closure_get as *const (),
]);
static CHOICE_CONFIRM_TABLE: FunctionTable = FunctionTable([
    closure_copy as *const (), closure_copy as *const (), choice_confirm_call as *const (),
    closure_target_type as *const (), closure_delete as *const (), closure_get as *const (),
]);
static OPTION_TABLE: FunctionTable = FunctionTable([
    closure_copy as *const (), closure_copy as *const (), option_call as *const (),
    closure_target_type as *const (), closure_delete as *const (), closure_get as *const (),
]);
static APPLY_TABLE: FunctionTable = FunctionTable([
    closure_copy as *const (), closure_copy as *const (), apply_call as *const (),
    closure_target_type as *const (), closure_delete as *const (), closure_get as *const (),
]);
static ON_COLOR_TABLE: FunctionTable = FunctionTable([
    closure_copy as *const (), closure_copy as *const (), on_color_call as *const (),
    closure_target_type as *const (), closure_delete as *const (), closure_get as *const (),
]);

// ---- the entry -------------------------------------------------------------------------------------------------------

/// The edit context's working paint block.
unsafe fn paint_block(screen: usize) -> usize {
    unsafe {
        let context = *((screen + SCREEN_EDIT_CONTEXT_OFFSET) as *const usize);
        *((context + EDIT_CONTEXT_PAINT_OFFSET) as *const usize)
    }
}

/// The row's second color, as stored (RGBA bytes, alpha 0xFF).
unsafe fn stored_color(screen: usize, row: usize) -> u32 {
    unsafe { rows::stored_second_color(paint_block(screen), row) }
}

/// A clip handle (0x60 bytes): a reference to a clip in the movie. It points to itself (+8, +0x10, +0x18), so it
/// must never move once constructed: it is constructed in place, used there and destroyed there.
#[repr(C, align(16))]
struct Handle([u8; 0x60]);

impl Handle {
    fn new() -> Self {
        Handle([0; 0x60])
    }

    /// Constructs, in place, the handle of `name` under `parent`. True if the clip exists. Destroy it either way.
    unsafe fn find(&mut self, parent: usize, name: &std::ffi::CStr) -> bool {
        type FindFn = unsafe extern "system" fn(usize, *mut Handle, *const std::ffi::c_char) -> *mut Handle;
        let find: FindFn = unsafe { std::mem::transmute(FIND_CLIP_ADDR) };
        let is_valid: unsafe extern "system" fn(*const Handle) -> bool = unsafe { std::mem::transmute(IS_VALID_CLIP_ADDR) };
        unsafe {
            find(parent, self, name.as_ptr());
            is_valid(self)
        }
    }

    fn address(&self) -> usize {
        self as *const Handle as usize
    }

    unsafe fn destroy(&mut self) {
        let destroy_value: unsafe extern "system" fn(usize) = unsafe { std::mem::transmute(DESTROY_HANDLE_VALUE_ADDR) };
        let destroy: unsafe extern "system" fn(usize) = unsafe { std::mem::transmute(DESTROY_HANDLE_ADDR) };
        unsafe {
            destroy_value(self.address() + 0x28);
            destroy(self.address());
        }
    }
}

/// Fills the item's clip: swatch and "R: G: B:" text. The clip's "Camouflage" (the paint pattern drawn over the
/// swatch: the stock entry sends it to a frame by the pattern's state, 0x140f452f0) is hidden: no pattern here.
unsafe extern "system" fn refresh_call(this: *const Closure, clip: usize) {
    let closure = unsafe { *this };
    let color = unsafe { stored_color(closure.screen, closure.row) };
    let [r, g, b, _] = color.to_le_bytes();
    unsafe {
        let mut swatch = Handle::new();
        let mut found = swatch.find(clip, c"Color/Tile");
        if !found {
            swatch.destroy();
            found = swatch.find(clip, c"Color");
        }
        if found {
            type SetColorFn = unsafe extern "system" fn(usize, u32, *const [*const i32; 2]) -> usize;
            let set_color: SetColorFn = std::mem::transmute(SET_COLOR_ADDR);
            static CHANNELS: [i32; 3] = [0, 1, 2];
            let range = [CHANNELS.as_ptr(), CHANNELS.as_ptr().add(3)];
            set_color(swatch.address() + 0x28, color, &range);
        }
        swatch.destroy();
        let mut pattern = Handle::new();
        if pattern.find(clip, c"Camouflage") {
            let set_visible: unsafe extern "system" fn(*mut Handle, u8) = std::mem::transmute(SET_VISIBLE_ADDR);
            set_visible(&mut pattern, 0);
        }
        pattern.destroy();
        let mut value = Handle::new();
        if value.find(clip, c"Value_0") {
            let text: Vec<u16> = format!("R:{r:>4}  G:{g:>4}  B:{b:>4}").encode_utf16().chain([0]).collect();
            let set_text: unsafe extern "system" fn(usize, *const u16) = std::mem::transmute(SET_TEXT_ADDR);
            set_text(value.address() + 8, text.as_ptr());
        }
        value.destroy();
    }
}

/// Opens the color picker; returns its job through `out`.
unsafe extern "system" fn confirm_call(this: *const Closure, out: *mut usize, a3: usize) -> *mut usize {
    let closure = unsafe { *this };
    log(&format!("second color: opening the picker for slot {}, a3 {a3:#x}", closure.row));
    let mut title = rows::Label::new();
    let mut on_color = Function::new();
    on_color.set(Closure { table: &ON_COLOR_TABLE, ..closure });
    type OpenPickerFn = unsafe extern "system" fn(usize, *mut usize, usize, *mut rows::Label, u32, *mut Function) -> *mut usize;
    unsafe {
        let Some(text) = rows::second_color_label(closure.row) else { return out };
        let title = rows::make_label(&mut title, text);
        let open: OpenPickerFn = std::mem::transmute(OPEN_PICKER_ADDR);
        let move_job: unsafe extern "system" fn(*mut usize, *mut usize) = std::mem::transmute(MOVE_JOB_ADDR);
        let mut job = 0usize;
        // a3 is the panel window; + 0x10 its stack of child windows, where the picker is pushed (as the game's
        // pattern colors and another Paint entry do; with null, the window job crashes when it pushes).
        if a3 == 0 {
            log("second color: no panel window to open the picker on");
            return out;
        }
        open(closure.screen, &mut job, a3 + 0x10, title, stored_color(closure.screen, closure.row), &mut on_color);
        move_job(out, &mut job);
    }
    out
}

/// The picker's callback, on every change: the color into the working paint block.
unsafe extern "system" fn on_color_call(this: *const Closure, color: *const u32) {
    let closure = unsafe { *this };
    unsafe { rows::store_second_color(paint_block(closure.screen), closure.row, *color) };
}

#[repr(C, align(16))]
struct RowBuffer([u8; 0x90]);

/// Appends the second color entry to the list description being built.
unsafe fn append_entry(screen: usize, description: usize, row: usize) {
    type MakeRowFn = unsafe extern "system" fn(*mut RowBuffer, *mut rows::Label) -> *mut RowBuffer;
    type AppendFn = unsafe extern "system" fn(usize, *mut RowBuffer, *mut Function, usize) -> usize;
    let mut label = rows::Label::new();
    let mut row_buffer = RowBuffer([0; 0x90]);
    let mut confirm = Function::new();
    confirm.set(Closure { table: &CONFIRM_TABLE, screen, row, index: 0 });
    let mut refresh = Function::new();
    refresh.set(Closure { table: &REFRESH_TABLE, screen, row, index: 0 });
    unsafe {
        let make_row: MakeRowFn = std::mem::transmute(MAKE_ROW_ADDR);
        let append: AppendFn = std::mem::transmute(APPEND_ITEM_ADDR);
        let swap: unsafe extern "system" fn(*mut Function, usize) = std::mem::transmute(SWAP_FUNCTION_ADDR);
        let destroy_row: unsafe extern "system" fn(*mut RowBuffer) = std::mem::transmute(DESTROY_ROW_ADDR);
        let Some(text) = rows::second_color_label(row) else { return };
        let label = rows::make_label(&mut label, text);
        make_row(&mut row_buffer, label);
        append(description, &mut row_buffer, &mut confirm, 0);
        let item = *((description + DESCRIPTION_ITEMS_END_OFFSET) as *const usize) - ITEM_SIZE;
        swap(&mut refresh, item + ITEM_REFRESH_OFFSET);
        destroy_row(&mut row_buffer);
    }
}

// ---- the choice entry: the builder's Material entry (second), taken over -------------------------------------------

const DESCRIPTION_ITEMS_BEGIN_OFFSET: usize = 0xc8;
const ITEM_ROW_OFFSET: usize = 0x10;
const ITEM_CONFIRM_OFFSET: usize = 0xf8;
const MAKE_DESCRIPTION_ADDR: usize = 0x140d3b1a0; // (description, &u32 0)
const DESTROY_DESCRIPTION_ADDR: usize = 0x140c83880;
const ADD_TITLE_ADDR: usize = 0x140d6a860; // (description + 0xb10, label)
const ADD_OPTION_ADDR: usize = 0x140c847e0; // (description, row, on-confirm (consumed), value) -> description
const RESTORE_JOB_ADDR: usize = 0x140f3c2b0; // (screen, job** out): puts the paint block back as it is now
const LIST_JOB_ADDR: usize = 0x140f382e0; // (description, job** out, host widget): the list window
const BRANCH_JOB_ADDR: usize = 0x1416c4660; // (job** a (borrowed), job** out, job** on failure (consumed))
const COPY_JOB_ADDR: usize = 0x1416c3510; // (job** from, job** to): add-ref copy
const SUB_WINDOW_JOB_ADDR: usize = 0x140f3b490; // (screen, job** out, job** inner (consumed)): "sub-window open" wrapper
const FUNCTION_JOB_ADDR: usize = 0x140a63060; // (job** out, std::function<void()> (consumed), result)
const UNREF_ADDR: usize = 0x1429235b0; // (refcount) -> the old count
const SCREEN_LIST_HOST_OFFSET: usize = 0x3d8;
const SCREEN_LIST_DATA_OFFSET: usize = 0x438;
const RESULT_SUCCESS: usize = 2;
static COMMAND_LIST: [u16; 19] = [
    b'0' as u16, b'1' as u16, b'_' as u16, b'0' as u16, b'7' as u16, b'0' as u16, b'_' as u16, b'C' as u16,
    b'o' as u16, b'm' as u16, b'm' as u16, b'a' as u16, b'n' as u16, b'd' as u16, b'L' as u16, b'i' as u16,
    b's' as u16, b't' as u16, 0,
];

#[repr(C, align(16))]
struct Description([u8; 0x1000]);

unsafe fn release_job(job: usize) {
    if job == 0 {
        return;
    }
    let unref: unsafe extern "system" fn(usize) -> i32 = unsafe { std::mem::transmute(UNREF_ADDR) };
    if unsafe { unref(job + 8) } == 1 {
        let delete: unsafe extern "system" fn(usize) = unsafe { std::mem::transmute(*(*(job as *const *const usize))) };
        unsafe { delete(job) };
    }
}

/// Destroys what a std::function holds (after a swap, the game's old function).
unsafe fn destroy_function(function: &mut Function) {
    let implementation = function.implementation as usize;
    if implementation != 0 {
        let table = unsafe { *(implementation as *const *const usize) };
        let delete: unsafe extern "system" fn(usize, u8) = unsafe { std::mem::transmute(*table.add(4)) };
        let on_heap = implementation != function.storage.as_ptr() as usize;
        unsafe { delete(implementation, on_heap as u8) };
        function.implementation = std::ptr::null_mut();
    }
}

/// The choice's name in the value box; the Material item's icon hidden.
unsafe extern "system" fn choice_refresh_call(this: *const Closure, clip: usize) {
    let closure = unsafe { *this };
    let (_, labels) = rows::choices_of(closure.row);
    let choice = unsafe { rows::stored_choice(paint_block(closure.screen), closure.row) } as usize;
    unsafe {
        let mut icon = Handle::new();
        if icon.find(clip, c"Material") {
            let set_visible: unsafe extern "system" fn(*mut Handle, u8) = std::mem::transmute(SET_VISIBLE_ADDR);
            set_visible(&mut icon, 0);
        }
        icon.destroy();
        let mut value = Handle::new();
        if value.find(clip, c"Value_0")
            && let Some(label) = labels.get(choice)
        {
            let set_text: unsafe extern "system" fn(usize, *const u16) = std::mem::transmute(SET_TEXT_ADDR);
            set_text(value.address() + 8, label.as_ptr());
        }
        value.destroy();
    }
}

/// Opens a list of the row's choices (the game's command list, as the pattern's Size entry does).
unsafe extern "system" fn choice_confirm_call(this: *const Closure, out: *mut usize, a3: usize) -> *mut usize {
    let closure = unsafe { *this };
    let (title, labels) = rows::choices_of(closure.row);
    log(&format!("choice entry: opening the list of {} options for row {}, a3 {a3:#x}", labels.len(), closure.row));
    let screen = closure.screen;
    let current = unsafe { rows::stored_choice(paint_block(screen), closure.row) } as usize;
    // a3 is the panel window; + 0x10 its stack of child windows, where the list's window is pushed (null crashes).
    if a3 == 0 {
        log("choice entry: no panel window to open the list on");
        return out;
    }
    let mut description = Description([0; 0x1000]);
    let d = description.0.as_mut_ptr() as usize;
    type MakeRowFn = unsafe extern "system" fn(*mut RowBuffer, *mut rows::Label) -> *mut RowBuffer;
    type AddOptionFn = unsafe extern "system" fn(usize, *mut RowBuffer, *mut Function, usize) -> usize;
    unsafe {
        let make: unsafe extern "system" fn(usize, *const u32) -> usize = std::mem::transmute(MAKE_DESCRIPTION_ADDR);
        let add_title: unsafe extern "system" fn(usize, *mut rows::Label) = std::mem::transmute(ADD_TITLE_ADDR);
        let make_row: MakeRowFn = std::mem::transmute(MAKE_ROW_ADDR);
        let add_option: AddOptionFn = std::mem::transmute(ADD_OPTION_ADDR);
        let destroy_row: unsafe extern "system" fn(*mut RowBuffer) = std::mem::transmute(DESTROY_ROW_ADDR);
        let zero = 0u32;
        make(d, &zero);
        *((d + 0x8) as *mut u32) = COMMAND_LIST.len() as u32 - 1;
        *((d + 0xc) as *mut u8) = 1;
        *((d + 0x10) as *mut *const u16) = COMMAND_LIST.as_ptr();
        // The window stack the list is pushed onto (read: the window job pushes onto it).
        *((d + 0xea0) as *mut usize) = a3 + 0x10;
        // Copied as the game's own option list does (only copied along; no reader found).
        *((d + 0xea8) as *mut u8) = 0;
        *((d + 0xa8) as *mut usize) = *((screen + SCREEN_LIST_DATA_OFFSET) as *const usize);
        // The window's clip handle is its own (read).
        *((d + 0xba3) as *mut u8) = 1;
        // Run an option's on-confirm on every cursor move: the preview (read).
        *((d + 0xba0) as *mut u8) = 1;
        let mut label = rows::Label::new();
        add_title(d + 0xb10, rows::make_label(&mut label, title));
        for (index, text) in labels.iter().enumerate() {
            let mut label = rows::Label::new();
            let mut row = RowBuffer([0; 0x90]);
            make_row(&mut row, rows::make_label(&mut label, text));
            let mut option = Function::new();
            option.set(Closure { table: &OPTION_TABLE, screen, row: closure.row, index });
            add_option(d, &mut row, &mut option, 0);
            let item = *((d + DESCRIPTION_ITEMS_END_OFFSET) as *const usize) - ITEM_SIZE;
            // The confirm sound request {id, priority, kind, name}: the game's default one.
            *((item + 0xa0) as *mut u32) = 100;
            *((item + 0xa4) as *mut u32) = 2;
            *((item + 0xa8) as *mut u32) = 1;
            *((item + 0xb0) as *mut usize) = 0x14390ba28;
            if index == current {
                *((d + 0xb8) as *mut u32) = index as u32;
            }
            destroy_row(&mut row);
        }
        let restore_job: unsafe extern "system" fn(usize, *mut usize) = std::mem::transmute(RESTORE_JOB_ADDR);
        let list_job: unsafe extern "system" fn(usize, *mut usize, usize) = std::mem::transmute(LIST_JOB_ADDR);
        let branch: unsafe extern "system" fn(*mut usize, *mut usize, *mut usize) = std::mem::transmute(BRANCH_JOB_ADDR);
        let copy: unsafe extern "system" fn(*mut usize, *mut usize) = std::mem::transmute(COPY_JOB_ADDR);
        let wrap: unsafe extern "system" fn(usize, *mut usize, *mut usize) = std::mem::transmute(SUB_WINDOW_JOB_ADDR);
        let destroy_description: unsafe extern "system" fn(usize) = std::mem::transmute(DESTROY_DESCRIPTION_ADDR);
        let (mut restore, mut list, mut branched, mut wrapped) = (0usize, 0usize, 0usize, 0usize);
        restore_job(screen, &mut restore);
        list_job(d, &mut list, screen + SCREEN_LIST_HOST_OFFSET);
        branch(&mut list, &mut branched, &mut restore);
        copy(&mut branched, &mut wrapped);
        wrap(screen, out, &mut wrapped);
        release_job(list);
        release_job(branched);
        destroy_description(d);
    }
    out
}

/// An option, chosen (or, with the cursor on it, previewed): a job that stores it.
unsafe extern "system" fn option_call(this: *const Closure, out: *mut usize) -> *mut usize {
    let closure = unsafe { *this };
    let mut apply = Function::new();
    apply.set(Closure { table: &APPLY_TABLE, ..closure });
    let job: unsafe extern "system" fn(*mut usize, *mut Function, usize) = unsafe { std::mem::transmute(FUNCTION_JOB_ADDR) };
    unsafe { job(out, &mut apply, RESULT_SUCCESS) };
    out
}

unsafe extern "system" fn apply_call(this: *const Closure) {
    let closure = unsafe { *this };
    unsafe { rows::store_choice(paint_block(closure.screen), closure.row, closure.index as u32) };
}

/// Makes the builder's second item (Material) the row's choice entry: its header, refresh and confirm.
unsafe fn take_over_material(screen: usize, description: usize, row: usize) {
    let begin = unsafe { *((description + DESCRIPTION_ITEMS_BEGIN_OFFSET) as *const usize) };
    let end = unsafe { *((description + DESCRIPTION_ITEMS_END_OFFSET) as *const usize) };
    if (end - begin) / ITEM_SIZE != 2 {
        log(&format!("choice entry: the panel has {} items, not Color and Material: left as it is", (end - begin) / ITEM_SIZE));
        return;
    }
    let item = begin + ITEM_SIZE;
    let (title, _) = rows::choices_of(row);
    unsafe {
        let make_row: unsafe extern "system" fn(usize, *mut rows::Label) -> usize = std::mem::transmute(MAKE_ROW_ADDR);
        let destroy_row: unsafe extern "system" fn(usize) = std::mem::transmute(DESTROY_ROW_ADDR);
        let swap: unsafe extern "system" fn(*mut Function, usize) = std::mem::transmute(SWAP_FUNCTION_ADDR);
        destroy_row(item + ITEM_ROW_OFFSET);
        let mut label = rows::Label::new();
        make_row(item + ITEM_ROW_OFFSET, rows::make_label(&mut label, title));
        for (offset, table) in [(ITEM_CONFIRM_OFFSET, &CHOICE_CONFIRM_TABLE), (ITEM_REFRESH_OFFSET, &CHOICE_REFRESH_TABLE)] {
            let mut function = Function::new();
            function.set(Closure { table, screen, row, index: 0 });
            swap(&mut function, item + offset);
            destroy_function(&mut function);
        }
    }
}

unsafe extern "system" fn add_color_entry_impl(context: usize, list: usize, slot: u8, label: usize) -> u64 {
    let result = unsafe { orig::<AddColorEntryFn>(&ORIG_ADD_COLOR_ENTRY)(context, list, slot, label) };
    if BUILDING.swap(false, Ordering::Relaxed) {
        PENDING[0].store(context, Ordering::Relaxed);
        PENDING[1].store(list, Ordering::Relaxed);
        PENDING[2].store(slot as usize, Ordering::Relaxed);
    }
    result
}

unsafe extern "system" fn pattern_check_impl(context: usize, out: usize, slot: u8) -> u64 {
    let description = PENDING[1].swap(0, Ordering::Relaxed);
    if description != 0 {
        let row_slot = PENDING[2].load(Ordering::Relaxed) as u8;
        if let Some(row) = rows::row_with_choices(row_slot) {
            let screen = PENDING[0].load(Ordering::Relaxed);
            unsafe {
                take_over_material(screen, description, row);
                append_entry(screen, description, row);
            }
        }
        // The check writes whether the AC has a paint pattern (a byte at `out`): with one, the builder adds Paint
        // Pattern's Apply entry next. Neither our rows nor DEVICE take a pattern (and our panel has three item
        // clips), so say no.
        let result = unsafe { orig::<PatternCheckFn>(&ORIG_PATTERN_CHECK)(context, out, slot) };
        unsafe { *(out as *mut u8) = 0 };
        return result;
    }
    unsafe { orig::<PatternCheckFn>(&ORIG_PATTERN_CHECK)(context, out, slot) }
}
