//! The panels of our rows with choices, and DEVICE's, built here (research: AC6_tools\Research\paint-popup,
//! OVERVIEW.md and wide2\report_*.md).
//!
//! A panel is a list window: a description filled with items (a header label, an on-confirm std::function returning
//! the job that opens what comes next, a refresh std::function filling the item's clip every frame), made into the
//! game's panel object (0x140f47160, as the Color / Material builder 0x140f49590 does) on a panel clip. Entry N fills
//! clip Item_N_0.
//! - Our rows: Color, the choices, a second color (if the row has one), all ours, on the rows' own panel clip (a copy
//!   of the Color / Material one with a color item third, tools/build_gfx.py).
//! - DEVICE: the game's own Color entry, and its brightness (its material, 90-99) as the game's Material entry would
//!   be for its slot (the stock material picker, which labels those materials by their glow), on the stock
//!   Color / Material clip. Built here rather than by the game's builder, which adds a Paint Pattern entry.
//! A color opens the game's color picker with our title, color and callback; a choice opens a list of options (the
//! game's command list, as the pattern's Size does). Both write into the edit context's working paint block, so the
//! picker and the list preview live and cancel puts the block back.

use std::sync::atomic::AtomicUsize;

use crate::ui::{self, Closure, Description, Function, FunctionTable, Handle, Label, with_clip};
use crate::{Hook, device, orig, rows};

// The rows' panel clip, in the Paint screen's MainContent clip (its handle at screen + 0x1da0). At screen setup
// (0x140f3fd90) the game looks its three panel clips up there by name and hides them; the setup hook does the same for
// ours (placed visible in the movie like the stock ones: the MainContent timeline replays its placements).
const ROWS_PANEL_NAME: &std::ffi::CStr = c"Color_Material_Color_Select";
const SCREEN_MAIN_CONTENT_OFFSET: usize = 0x1da0;
static ORIG_SET_UP_PANELS: AtomicUsize = AtomicUsize::new(0);

const SCREEN_EDIT_CONTEXT_OFFSET: usize = 0x440;
const EDIT_CONTEXT_CATEGORY_OFFSET: usize = 0x28;
const EDIT_CONTEXT_PAINT_OFFSET: usize = 0x40;
// The list window hosting option lists, and a value its description takes from the screen.
const SCREEN_LIST_HOST_OFFSET: usize = 0x3d8;
const SCREEN_LIST_DATA_OFFSET: usize = 0x438;
// The Color / Material panel clip's handle in the panel list (list = the Paint list's panel state; list[0] the screen,
// list + 8 the owner the panels get).
const LIST_STOCK_PANEL_OFFSET: usize = 0x78;
const LIST_OWNER_OFFSET: usize = 8;
// The game's "Color" header text.
const COLOR_TEXT: u32 = 0x334e6;

pub(crate) fn hooks() -> [Hook; 1] {
    // mov [rsp+10],rdx / mov [rsp+8],rcx
    [(0x140f3fd90, [0x48, 0x89, 0x54, 0x24, 0x10, 0x48], set_up_panels_impl as *const (), &ORIG_SET_UP_PANELS)]
}

type SetUpPanelsFn = unsafe extern "system" fn(usize, usize, usize, usize) -> usize;

unsafe extern "system" fn set_up_panels_impl(list: usize, content: usize, screen: usize, a4: usize) -> usize {
    let result = unsafe { orig::<SetUpPanelsFn>(&ORIG_SET_UP_PANELS)(list, content, screen, a4) };
    unsafe { with_clip(screen + SCREEN_MAIN_CONTENT_OFFSET, ROWS_PANEL_NAME, |panel| panel.set_visible(false)) };
    result
}

// ---- what an entry is about ----------------------------------------------------------------------------------------

/// DEVICE, where an entry's `row` would be.
const DEVICE: usize = usize::MAX;

/// What our std::functions capture.
#[derive(Clone, Copy)]
struct Entry {
    screen: usize,
    /// One of our rows, or DEVICE.
    row: usize,
    /// A color: which (0 the row's color, 1 its second color). An option: its index.
    value: usize,
}

impl Entry {
    unsafe fn context(&self) -> usize {
        unsafe { *((self.screen + SCREEN_EDIT_CONTEXT_OFFSET) as *const usize) }
    }

    /// The edit context's working paint block.
    unsafe fn paint(&self) -> usize {
        unsafe { *((self.context() + EDIT_CONTEXT_PAINT_OFFSET) as *const usize) }
    }

    /// DEVICE's paint sections of the selected parts (0x14045d960(out, category)).
    unsafe fn sections(&self) -> [usize; 2] {
        let sections_of: unsafe extern "system" fn(*mut [usize; 2], u8) -> usize = unsafe { std::mem::transmute(0x14045d960usize) };
        let mut sections = [0; 2];
        unsafe { sections_of(&mut sections, *((self.context() + EDIT_CONTEXT_CATEGORY_OFFSET) as *const u8)) };
        sections
    }

    unsafe fn color(&self) -> u32 {
        unsafe { rows::stored_color(self.paint(), self.row, self.value) }
    }

    fn choices(&self) -> (&'static [u16], Vec<&'static [u16]>) {
        rows::choices(self.row).unwrap_or((&[0], Vec::new()))
    }

    unsafe fn choice(&self) -> usize {
        unsafe { rows::stored_choice(self.paint(), self.row) as usize }
    }

    unsafe fn store_choice(&self, index: usize) {
        unsafe { rows::store_choice(self.paint(), self.row, index as u32) };
    }

    /// DEVICE's material (0x14045a980(paint, sections, slot)).
    unsafe fn device_material(&self) -> u16 {
        let read: unsafe extern "system" fn(usize, *const [usize; 2], u8) -> u16 = unsafe { std::mem::transmute(0x14045a980usize) };
        unsafe { read(self.paint(), &self.sections(), device::SLOT) }
    }

    fn closure(self, table: &'static FunctionTable) -> Closure<Entry> {
        Closure { table, captures: self }
    }
}

static COLOR_REFRESH: FunctionTable = ui::table::<Entry>(color_refresh as *const ());
static COLOR_CONFIRM: FunctionTable = ui::table::<Entry>(color_confirm as *const ());
static ON_COLOR: FunctionTable = ui::table::<Entry>(on_color as *const ());
static CHOICE_REFRESH: FunctionTable = ui::table::<Entry>(choice_refresh as *const ());
static CHOICE_CONFIRM: FunctionTable = ui::table::<Entry>(choice_confirm as *const ());
static OPTION_CONFIRM: FunctionTable = ui::table::<Entry>(option_confirm as *const ());
static OPTION_APPLY: FunctionTable = ui::table::<Entry>(option_apply as *const ());
static BRIGHTNESS_REFRESH: FunctionTable = ui::table::<Entry>(brightness_refresh as *const ());
static BRIGHTNESS_CONFIRM: FunctionTable = ui::table::<Entry>(brightness_confirm as *const ());

/// The stack a sub-window (picker, option list) is pushed on: the on-confirm's `a3` is the panel window, + 0x10 its
/// child windows (as the game's own entries pass it). Null would crash when the window is pushed.
fn child_windows(a3: usize) -> Option<usize> {
    (a3 != 0).then_some(a3 + 0x10)
}

// ---- colors ----------------------------------------------------------------------------------------------------------

/// The swatch and "R: G: B:" text, with no pattern over the swatch.
unsafe extern "system" fn color_refresh(this: *const Closure<Entry>, clip: usize) {
    let entry = unsafe { (*this).captures };
    let color = unsafe { entry.color() };
    let [r, g, b, _] = color.to_le_bytes();
    let text: Vec<u16> = format!("R:{r:>4}  G:{g:>4}  B:{b:>4}").encode_utf16().chain([0]).collect();
    unsafe {
        // As the game's Color entry: "Color/Tile" if there is one, else "Color" itself (the panel's case).
        let mut tile = false;
        with_clip(clip, c"Color/Tile", |swatch| { swatch.set_color(color); tile = true; });
        if !tile {
            with_clip(clip, c"Color", |swatch| swatch.set_color(color));
        }
        ui::show_no_pattern(clip, *((entry.context() + EDIT_CONTEXT_CATEGORY_OFFSET) as *const u8));
        with_clip(clip, c"Value_0", |value| value.set_text(text.as_ptr()));
    }
}

/// Opens the game's color picker (0x140f37450(screen, job** out, child windows, title, color, on-color)).
unsafe extern "system" fn color_confirm(this: *const Closure<Entry>, out: *mut usize, a3: usize) -> *mut usize {
    let entry = unsafe { (*this).captures };
    let Some(windows) = child_windows(a3) else { return out };
    let open: unsafe extern "system" fn(usize, *mut usize, usize, *mut Label, u32, *mut Function) -> usize =
        unsafe { std::mem::transmute(0x140f37450usize) };
    let move_job: unsafe extern "system" fn(*mut usize, *mut usize) = unsafe { std::mem::transmute(0x140c81f20usize) };
    let mut title = Label::new();
    let mut on_color = Function::new();
    let mut job = 0;
    unsafe {
        open(entry.screen, &mut job, windows, title.set(rows::color_label(entry.row, entry.value)), entry.color(),
             on_color.set(entry.closure(&ON_COLOR)));
        move_job(out, &mut job);
    }
    out
}

/// The picker's callback, on every change.
unsafe extern "system" fn on_color(this: *const Closure<Entry>, color: *const u32) {
    let entry = unsafe { (*this).captures };
    unsafe { rows::store_color(entry.paint(), entry.row, entry.value, *color) };
}

// ---- choices ---------------------------------------------------------------------------------------------------------

/// The choice's name in the value box; the item's material icon hidden.
unsafe extern "system" fn choice_refresh(this: *const Closure<Entry>, clip: usize) {
    let entry = unsafe { (*this).captures };
    let (_, labels) = entry.choices();
    let Some(label) = labels.get(unsafe { entry.choice() }) else { return };
    unsafe {
        with_clip(clip, c"Material", |icon| icon.set_visible(false));
        with_clip(clip, c"Value_0", |value| value.set_text(label.as_ptr()));
    }
}

/// Opens the list of options, the current one selected (as the pattern's Size does, 0x140f36ef0).
unsafe extern "system" fn choice_confirm(this: *const Closure<Entry>, out: *mut usize, a3: usize) -> *mut usize {
    let entry = unsafe { (*this).captures };
    let Some(windows) = child_windows(a3) else { return out };
    let (title, labels) = entry.choices();
    let restore_job: unsafe extern "system" fn(usize, *mut usize) = unsafe { std::mem::transmute(0x140f3c2b0usize) };
    let list_job: unsafe extern "system" fn(usize, *mut usize, usize) = unsafe { std::mem::transmute(0x140f382e0usize) };
    let on_failure: unsafe extern "system" fn(*mut usize, *mut usize, *mut usize) = unsafe { std::mem::transmute(0x1416c4660usize) };
    let sub_window: unsafe extern "system" fn(usize, *mut usize, *mut usize) = unsafe { std::mem::transmute(0x140f3b490usize) };
    unsafe {
        let mut description = Description::new();
        description.write(0xea0, windows);
        description.write(0xa8, *((entry.screen + SCREEN_LIST_DATA_OFFSET) as *const usize));
        // Run an option's on-confirm with the cursor on it: the preview.
        description.write(0xba0, 1u8);
        description.add_title(title);
        for (index, text) in labels.iter().enumerate() {
            let mut label = Label::new();
            let mut option = Function::new();
            let item = description.add_option(label.set(text),option.set(Entry { value: index, ..entry }.closure(&OPTION_CONFIRM)));
            // The confirm sound request {id, priority, kind, name}: the game's default one.
            *((item + 0xa0) as *mut [u32; 3]) = [100, 2, 1];
            *((item + 0xb0) as *mut usize) = 0x14390ba28;
        }
        description.write(0xb8, entry.choice() as u32);
        // The list window (on the screen's list host), then on failure (cancel) the paint block as it is now, all
        // inside the "sub-window open" wrapper. The combinators take the references they're given (restore, list
        // with failure), except the list window job itself, released here.
        let (mut restore, mut list, mut list_with_failure) = (0, 0, 0);
        restore_job(entry.screen, &mut restore);
        list_job(description.address(), &mut list, entry.screen + SCREEN_LIST_HOST_OFFSET);
        on_failure(&mut list, &mut list_with_failure, &mut restore);
        sub_window(entry.screen, out, &mut list_with_failure);
        ui::release(list);
    }
    out
}

/// An option, chosen or previewed: a job that stores it.
unsafe extern "system" fn option_confirm(this: *const Closure<Entry>, out: *mut usize) -> *mut usize {
    let entry = unsafe { (*this).captures };
    let mut apply = Function::new();
    unsafe { ui::function_job(out, apply.set(entry.closure(&OPTION_APPLY))) };
    out
}

unsafe extern "system" fn option_apply(this: *const Closure<Entry>) {
    let entry = unsafe { (*this).captures };
    unsafe { entry.store_choice(entry.value) };
}

// ---- DEVICE's brightness: the game's material entry, for its slot ----------------------------------------------------

/// The material's icon and text, as the game's material entry does (0x140f47e80).
unsafe extern "system" fn brightness_refresh(this: *const Closure<Entry>, clip: usize) {
    let entry = unsafe { (*this).captures };
    let material = unsafe { entry.device_material() };
    let material_row: unsafe extern "system" fn(u32) -> usize = unsafe { std::mem::transmute(0x140e0cdd0usize) };
    let text: unsafe extern "system" fn(*const u8, *mut Label) -> *mut Label = unsafe { std::mem::transmute(0x140f35c40usize) };
    unsafe {
        let mut label = Label::new();
        // What the text function reads: the slot (u8 at + 0), the material's param row (+ 0x10), the material (+ 0x20).
        let mut words = [0usize; 5];
        *(words.as_mut_ptr() as *mut u8) = device::SLOT;
        words[2] = material_row(material as u32);
        words[4] = material as usize;
        text(words.as_ptr() as *const u8, &mut label);
        with_clip(clip, c"Value_0", |value| value.set_text(label.text()));
        label.destroy();
        let mut handle = Handle::new();
        if handle.find(clip, c"Material") {
            #[repr(C, align(16))]
            struct Icon([u8; 0x180]);
            let make: unsafe extern "system" fn(*mut Icon, *mut Handle) = std::mem::transmute(0x140f44fd0usize);
            let show: unsafe extern "system" fn(*mut Icon, u16) = std::mem::transmute(0x140f45260usize);
            let destroy_extra: unsafe extern "system" fn(usize) = std::mem::transmute(0x140f229c0usize);
            let mut icon = Icon([0; 0x180]);
            make(&mut icon, &mut handle);
            show(&mut icon, material);
            let at = &mut icon as *mut Icon as usize;
            destroy_extra(at + 0x60);
            ui::destroy_handle(at);
        }
        handle.destroy();
    }
}

/// Opens the game's material picker for DEVICE (0x140f38320(screen, out, child windows, slot)), as the game's
/// material entry does.
unsafe extern "system" fn brightness_confirm(this: *const Closure<Entry>, out: *mut usize, a3: usize) -> *mut usize {
    let entry = unsafe { (*this).captures };
    let Some(windows) = child_windows(a3) else { return out };
    let open: unsafe extern "system" fn(usize, *mut usize, usize, u8) = unsafe { std::mem::transmute(0x140f38320usize) };
    unsafe { open(entry.screen, out, windows, device::SLOT) };
    out
}

// ---- the panel -------------------------------------------------------------------------------------------------------

/// Builds the panel of one of our rows with choices (`row`), or of DEVICE (None), for the Paint list's panel state
/// `list`. Returns the panel object (one reference, the constructor's).
pub(crate) unsafe fn build(list: usize, row: Option<usize>) -> usize {
    // Two screens: the Paint list's (list[0]: its movie clips; no edit context), and the one the owner gives the
    // entries (its edit context at + 0x440), as the game's builder does (owner virtuals 1 and 2; checked: the game
    // passes its own Color entry the same object).
    let list_screen = unsafe { *(list as *const usize) };
    let owner = list + LIST_OWNER_OFFSET;
    let owner_fn = |index: usize| -> unsafe extern "system" fn(usize) -> usize {
        unsafe { std::mem::transmute(*(*(owner as *const *const usize)).add(index)) }
    };
    let screen = unsafe { owner_fn(2)(owner) };
    let entry = Entry { screen, row: row.unwrap_or(DEVICE), value: 0 };
    unsafe {
        let mut description = Description::new();
        description.write(0xa8, owner_fn(1)(owner));
        let add = |description: &mut Description, label: *mut Label, confirm: &'static FunctionTable, refresh: &'static FunctionTable, value| {
            let mut function = Function::new();
            let item = description.add_item(label, function.set(Entry { value, ..entry }.closure(confirm)));
            ui::set_function(item + ui::ITEM_REFRESH_OFFSET, Entry { value, ..entry }.closure(refresh));
        };
        let mut color_label = Label::new();
        let mut choices_label = Label::new();
        match row {
            Some(row) => {
                add(&mut description, color_label.set_game_text(COLOR_TEXT), &COLOR_CONFIRM, &COLOR_REFRESH, 0);
                if let Some((title, _)) = rows::choices(row) {
                    add(&mut description, choices_label.set(title), &CHOICE_CONFIRM, &CHOICE_REFRESH, 0);
                }
                if rows::has_second_color(row) {
                    let mut label = Label::new();
                    add(&mut description, label.set(rows::color_label(row, 1)), &COLOR_CONFIRM, &COLOR_REFRESH, 1);
                }
            }
            None => {
                // The game's own Color entry: 0x140f3b1c0(screen, description, slot, label).
                let add_color: unsafe extern "system" fn(usize, usize, u8, *mut Label) = std::mem::transmute(0x140f3b1c0usize);
                add_color(screen, description.address(), device::SLOT, color_label.set_game_text(COLOR_TEXT));
                add(&mut description, choices_label.set(device::title()), &BRIGHTNESS_CONFIRM, &BRIGHTNESS_REFRESH, 0);
            }
        }
        // The panel clip's handle; the constructor gets a copy of it (and keeps that).
        let mut clip = Handle::new();
        match row {
            Some(_) => {
                clip.find(list_screen + SCREEN_MAIN_CONTENT_OFFSET, ROWS_PANEL_NAME);
            }
            None => {
                clip.copy(list + LIST_STOCK_PANEL_OFFSET);
            }
        }
        let allocate: unsafe extern "system" fn(usize, usize, usize) -> usize = std::mem::transmute(0x142923280usize);
        let construct: unsafe extern "system" fn(usize, *mut Handle, *mut Description, usize) -> usize = std::mem::transmute(0x140f47160usize);
        let panel = allocate(0x1da8, 8, *(0x144e2e638 as *const usize));
        if panel != 0 {
            let mut copy = Handle::new();
            construct(panel, copy.copy(clip.address()), &mut *description, owner);
        }
        clip.destroy();
        panel
    }
}
