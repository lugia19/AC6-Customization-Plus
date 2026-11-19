//! The game's menu plumbing, as the Paint screen code uses it (research: AC6_tools\Research\paint-popup, OVERVIEW.md
//! and wide\ / wide2\ report_*.md): text labels, clip handles, std::functions, jobs and list descriptions.

/// UTF-16 with a terminator, at compile time (ASCII only).
pub(crate) const fn utf16<const N: usize>(text: &[u8; N]) -> [u16; N] {
    let mut out = [0; N];
    let mut i = 0;
    while i < N {
        out[i] = text[i] as u16;
        i += 1;
    }
    out
}

/// UTF-16 with a terminator, kept forever: the game's labels hold the text's address.
pub(crate) fn leak_utf16(text: &str) -> &'static [u16] {
    text.encode_utf16().chain([0]).collect::<Vec<u16>>().leak()
}

unsafe fn game_fn<F: Copy>(address: usize) -> F {
    unsafe { std::mem::transmute_copy(&address) }
}

// ---- labels ----------------------------------------------------------------------------------------------------------

/// A label (0x40 bytes): a text, as items and titles take it. It holds the text's address, not a copy.
#[repr(C, align(16))]
pub(crate) struct Label([usize; 8]);

#[repr(C)]
struct TextId {
    category: i16,
    id: i32,
}

// Menu text category 200, no ID: the label only carries it along.
static LABEL_TEXT_ID: TextId = TextId { category: 200, id: 0 };

impl Label {
    pub(crate) fn new() -> Self {
        Label([0; 8])
    }

    /// Our text. (0x140cb2970(out, text, text ID): what the game's text lookup ends in.)
    pub(crate) unsafe fn set(&mut self, text: &'static [u16]) -> *mut Label {
        let make: unsafe extern "system" fn(*mut Label, *const u16, *const TextId) -> *mut Label = unsafe { game_fn(0x140cb2970) };
        unsafe { make(self, text.as_ptr(), &LABEL_TEXT_ID) }
    }

    /// One of the game's menu texts (category 200).
    pub(crate) unsafe fn set_game_text(&mut self, id: u32) -> *mut Label {
        let lookup: unsafe extern "system" fn(*mut Label, u32) -> *mut Label = unsafe { game_fn(0x140caf6a0) };
        unsafe { lookup(self, id) }
    }

    /// The text: the address it holds (+0), else its own string (+0x10; inline up to 7 characters, capacity +0x28).
    pub(crate) fn text(&self) -> *const u16 {
        match self.0 {
            [0, _, _, _, _, capacity, _, _] if capacity > 7 => self.0[2] as *const u16,
            [0, ..] => &self.0[2] as *const usize as *const u16,
            [address, ..] => address as *const u16,
        }
    }

    /// Frees its own string, if on the heap (as the game does inline: allocator +8, slot 0x68).
    pub(crate) unsafe fn destroy(&mut self) {
        if self.0[5] > 7 {
            let allocator = self.0[1];
            let free: unsafe extern "system" fn(usize, usize) = unsafe { game_fn(*((*(allocator as *const usize)) as *const usize).add(0x68 / 8)) };
            unsafe { free(allocator, self.0[2]) };
            self.0[5] = 7;
        }
    }
}

// ---- clip handles ----------------------------------------------------------------------------------------------------

/// A clip handle (0x60 bytes): a reference to a clip in a movie. It points to itself (+8, +0x10, +0x18), so it must
/// never move once constructed: it is constructed in place, used there and destroyed there.
#[repr(C, align(16))]
pub(crate) struct Handle([u8; 0x60]);

const HANDLE_VALUE_OFFSET: usize = 0x28;

impl Handle {
    pub(crate) fn new() -> Self {
        Handle([0; 0x60])
    }

    pub(crate) fn address(&mut self) -> usize {
        self as *mut Handle as usize
    }

    /// Constructs, in place, the handle of the clip `name` (a path: "A/B") under `parent` (a handle's address).
    /// True if it exists. Destroy it either way.
    pub(crate) unsafe fn find(&mut self, parent: usize, name: &std::ffi::CStr) -> bool {
        let find: unsafe extern "system" fn(usize, *mut Handle, *const std::ffi::c_char) -> *mut Handle = unsafe { game_fn(0x140c928e0) };
        let is_valid: unsafe extern "system" fn(*mut Handle) -> bool = unsafe { game_fn(0x140c6a500) };
        unsafe {
            find(parent, self, name.as_ptr());
            is_valid(self)
        }
    }

    /// Constructs, in place, a copy of the handle at `from`.
    pub(crate) unsafe fn copy(&mut self, from: usize) -> *mut Handle {
        let copy: unsafe extern "system" fn(*mut Handle, usize) -> *mut Handle = unsafe { game_fn(0x140c92e20) };
        unsafe { copy(self, from) }
    }

    pub(crate) unsafe fn destroy(&mut self) {
        unsafe { destroy_handle(self.address()) };
    }

    pub(crate) unsafe fn set_visible(&mut self, visible: bool) {
        let set: unsafe extern "system" fn(*mut Handle, u8) = unsafe { game_fn(0x140c6aab0) };
        unsafe { set(self, visible as u8) };
    }

    /// Sets a text field's text (the game copies it).
    pub(crate) unsafe fn set_text(&mut self, text: *const u16) {
        let set: unsafe extern "system" fn(usize, *const u16) = unsafe { game_fn(0x140c8cba0) };
        unsafe { set(self.address() + 8, text) };
    }

    /// Sets a clip's color (a stored paint color: RGBA bytes).
    pub(crate) unsafe fn set_color(&mut self, color: u32) {
        static CHANNELS: [i32; 3] = [0, 1, 2];
        let set: unsafe extern "system" fn(usize, u32, *const [*const i32; 2]) -> usize = unsafe { game_fn(0x141614a80) };
        let channels = CHANNELS.as_ptr_range();
        unsafe { set(self.address() + HANDLE_VALUE_OFFSET, color, &[channels.start, channels.end]) };
    }
}

/// Destroys the handle at `handle`: its value, then the handle.
pub(crate) unsafe fn destroy_handle(handle: usize) {
    let destroy_value: unsafe extern "system" fn(usize) = unsafe { game_fn(0x14160f3d0) };
    let destroy: unsafe extern "system" fn(usize) = unsafe { game_fn(0x140c6a370) };
    unsafe {
        destroy_value(handle + HANDLE_VALUE_OFFSET);
        destroy(handle);
    }
}

/// A swatch's "Camouflage" (the paint pattern over it) as "no pattern", for the selected part category: what the
/// game's swatch refreshes do for a slot the pattern can't apply to (state 0 from 0x140e0c660 -> 0x140f452f0). Never
/// hide it: a hidden one broke the pattern thumbnails and overlays of the whole Paint screen.
pub(crate) unsafe fn show_no_pattern(swatch_clip: usize, category: u8) {
    let set: unsafe extern "system" fn(*mut Handle, u8, *const u8) = unsafe { game_fn(0x140f452f0) };
    let not_applied = 0u8;
    unsafe { with_clip(swatch_clip, c"Camouflage", |pattern| set(pattern, category, &not_applied)) };
}

/// Finds `name` under `parent`, does `act` with it if it exists, destroys the handle.
pub(crate) unsafe fn with_clip(parent: usize, name: &std::ffi::CStr, act: impl FnOnce(&mut Handle)) {
    let mut handle = Handle::new();
    unsafe {
        if handle.find(parent, name) {
            act(&mut handle);
        }
        handle.destroy();
    }
}

// ---- std::function (MSVC: 0x40 bytes, the implementation inline at +0, a pointer to it at +0x38) ------------------

/// Our std::functions' implementation: a table and what they capture (the same for all of them).
#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct Closure<T: Copy + 'static> {
    pub(crate) table: &'static FunctionTable,
    pub(crate) captures: T,
}

/// An implementation's table: copy, move, call, target type, delete, get.
#[repr(C)]
pub(crate) struct FunctionTable([*const (); 6]);

// Read-only after construction.
unsafe impl Sync for FunctionTable {}

/// The table of a closure whose captures are `T` and whose call is `call` (its first argument: the closure).
pub(crate) const fn table<T: Copy + 'static>(call: *const ()) -> FunctionTable {
    FunctionTable([
        closure_copy::<T> as *const (),
        closure_copy::<T> as *const (),
        call,
        closure_target_type as *const (),
        closure_delete as *const (),
        closure_get as *const (),
    ])
}

/// Captures that hold a reference on a reference-counted game object (refcount at + 8), as the game's closures
/// capturing the edit context do.
pub(crate) trait HoldsReference {
    fn referenced(&self) -> usize;
}

/// The table of a closure holding a reference through its captures: every copy (and move: MSVC deletes the source
/// after) adds one, delete releases one. Whoever makes the first closure adds its reference (`add_reference`).
pub(crate) const fn table_holding<T: Copy + HoldsReference + 'static>(call: *const ()) -> FunctionTable {
    FunctionTable([
        closure_copy_holding::<T> as *const (),
        closure_copy_holding::<T> as *const (),
        call,
        closure_target_type as *const (),
        closure_delete_holding::<T> as *const (),
        closure_get as *const (),
    ])
}

unsafe extern "system" fn closure_copy_holding<T: Copy + HoldsReference + 'static>(this: *const Closure<T>, to: *mut Closure<T>) -> *mut Closure<T> {
    unsafe {
        add_reference((*this).captures.referenced());
        closure_copy(this, to)
    }
}

unsafe extern "system" fn closure_delete_holding<T: Copy + HoldsReference + 'static>(this: *const Closure<T>, _deallocate: u8) {
    unsafe { release((*this).captures.referenced()) };
}

unsafe extern "system" fn closure_copy<T: Copy + 'static>(this: *const Closure<T>, to: *mut Closure<T>) -> *mut Closure<T> {
    unsafe { to.write(*this) };
    to
}

unsafe extern "system" fn closure_target_type(_this: usize) -> usize {
    static TYPE: u8 = 0;
    &TYPE as *const u8 as usize
}

unsafe extern "system" fn closure_delete(_this: usize, _deallocate: u8) {}

unsafe extern "system" fn closure_get(this: usize) -> usize {
    this + 8
}

#[repr(C, align(16))]
pub(crate) struct Function {
    storage: [usize; 7],
    implementation: usize,
}

impl Function {
    /// Empty; filled in place (the pointer at +0x38 points into the object itself).
    pub(crate) fn new() -> Self {
        Function { storage: [0; 7], implementation: 0 }
    }

    pub(crate) fn set<T: Copy + 'static>(&mut self, closure: Closure<T>) -> *mut Function {
        const { assert!(size_of::<Closure<T>>() <= 0x38) };
        let at = self.storage.as_mut_ptr() as *mut Closure<T>;
        unsafe { at.write(closure) };
        self.implementation = at as usize;
        self
    }

    /// Destroys what it holds (after a swap, possibly the game's).
    pub(crate) unsafe fn destroy(&mut self) {
        if self.implementation != 0 {
            let table = unsafe { *(self.implementation as *const *const usize) };
            let delete: unsafe extern "system" fn(usize, u8) = unsafe { game_fn(*table.add(4)) };
            let on_heap = self.implementation != self.storage.as_ptr() as usize;
            unsafe { delete(self.implementation, on_heap as u8) };
            self.implementation = 0;
        }
    }
}

/// Puts `closure` into the std::function at `slot` (an item's), destroying what was there.
pub(crate) unsafe fn set_function<T: Copy + 'static>(slot: usize, closure: Closure<T>) {
    let swap: unsafe extern "system" fn(*mut Function, usize) = unsafe { game_fn(0x140f3e380) };
    let mut function = Function::new();
    unsafe {
        swap(function.set(closure), slot);
        function.destroy();
    }
}

// ---- reference-counted objects (jobs, the edit context: refcount at + 8) ------------------------------------------

pub(crate) unsafe fn add_reference(object: usize) {
    let add: unsafe extern "system" fn(usize) = unsafe { game_fn(0x142923570) };
    unsafe { add(object + 8) };
}

pub(crate) unsafe fn release(object: usize) {
    if object == 0 {
        return;
    }
    let unref: unsafe extern "system" fn(usize) -> i32 = unsafe { game_fn(0x1429235b0) };
    if unsafe { unref(object + 8) } == 1 {
        let delete: unsafe extern "system" fn(usize) = unsafe { game_fn(**(object as *const *const usize)) };
        unsafe { delete(object) };
    }
}

/// A job that runs `function` (std::function<void()>, consumed) and finishes with success.
pub(crate) unsafe fn function_job(out: *mut usize, function: *mut Function) {
    const SUCCESS: usize = 2;
    let make: unsafe extern "system" fn(*mut usize, *mut Function, usize) = unsafe { game_fn(0x140a63060) };
    unsafe { make(out, function, SUCCESS) };
}

// ---- list descriptions -------------------------------------------------------------------------------------------

/// A list window's description (0xEB0 bytes used): its clip, title, items (0x308 bytes each) and settings.
#[repr(C, align(16))]
pub(crate) struct Description([u8; 0x1000]);

const DESCRIPTION_ITEMS_END_OFFSET: usize = 0xd0;
const ITEM_SIZE: usize = 0x308;
pub(crate) const ITEM_REFRESH_OFFSET: usize = 0x1b8;
// Both kinds of list window here use the game's generic list clip.
static COMMAND_LIST: [u16; 19] = utf16(b"01_070_CommandList\0");

impl Description {
    /// A new description (0x140d3b1a0) of a list window on the command list clip.
    pub(crate) unsafe fn new() -> Box<Description> {
        let mut description = Box::new(Description([0; 0x1000]));
        let make: unsafe extern "system" fn(*mut Description, *const u32) -> usize = unsafe { game_fn(0x140d3b1a0) };
        let zero = 0u32;
        unsafe {
            make(&mut *description, &zero);
            description.write(0x8, COMMAND_LIST.len() as u32 - 1);
            description.write(0xc, 1u8);
            description.write(0x10, COMMAND_LIST.as_ptr());
            // The window's clip handle is its own.
            description.write(0xba3, 1u8);
        }
        description
    }

    pub(crate) fn address(&mut self) -> usize {
        self as *mut Description as usize
    }

    pub(crate) unsafe fn write<T>(&mut self, offset: usize, value: T) {
        unsafe { ((self.address() + offset) as *mut T).write_unaligned(value) };
    }

    pub(crate) unsafe fn add_title(&mut self, text: &'static [u16]) {
        let add: unsafe extern "system" fn(usize, *mut Label) = unsafe { game_fn(0x140d6a860) };
        let mut label = Label::new();
        unsafe { add(self.address() + 0xb10, label.set(text)) };
    }

    /// Appends a panel entry (0x140d3c600, as the panel builders do); returns the item.
    pub(crate) unsafe fn add_item(&mut self, label: *mut Label, confirm: *mut Function) -> usize {
        unsafe { add_item(self.address(), label, confirm) }
    }

    /// Appends an option of an option list (0x140c847e0, as the game's own lists do: with 0x140d3c600 confirming
    /// an option leaves the list open); returns the item.
    pub(crate) unsafe fn add_option(&mut self, label: *mut Label, confirm: *mut Function) -> usize {
        unsafe { append(self.address(), 0x140c847e0, label, confirm) }
    }
}

/// Appends an entry to a description the game made (as its row builder and panel builders do, 0x140d3c600);
/// returns the item.
pub(crate) unsafe fn add_item(description: usize, label: *mut Label, confirm: *mut Function) -> usize {
    unsafe { append(description, 0x140d3c600, label, confirm) }
}

/// 0x140d92d80 makes the label's row (0x88 bytes), `add` appends the item and consumes the function, 0x140c82fd0
/// destroys the row.
unsafe fn append(description: usize, add: usize, label: *mut Label, confirm: *mut Function) -> usize {
    #[repr(C, align(16))]
    struct Row([u8; 0x90]);
    let make_row: unsafe extern "system" fn(*mut Row, *mut Label) -> *mut Row = unsafe { game_fn(0x140d92d80) };
    let append: unsafe extern "system" fn(usize, *mut Row, *mut Function, usize) -> usize = unsafe { game_fn(add) };
    let destroy_row: unsafe extern "system" fn(*mut Row) = unsafe { game_fn(0x140c82fd0) };
    let mut row = Row([0; 0x90]);
    unsafe {
        make_row(&mut row, label);
        append(description, &mut row, confirm, 0);
        destroy_row(&mut row);
        *((description + DESCRIPTION_ITEMS_END_OFFSET) as *const usize) - ITEM_SIZE
    }
}

impl Drop for Description {
    fn drop(&mut self) {
        let destroy: unsafe extern "system" fn(*mut Description) = unsafe { game_fn(0x140c83880) };
        unsafe { destroy(self) };
    }
}
