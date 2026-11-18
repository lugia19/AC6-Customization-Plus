//! The HUD rows in the Paint screen (rows.rs): HUD (a color, and an opacity as its choice) and HUD
//! WARNING (a color). The stock base color (#DFFCFF), red and fully opaque are the stock HUD.
//!
//! The package has a prepared copy of the HUD movie (menu/01_000_fe.gfx, tools/prepare_hud.py), which hud_movie.rs
//! rewrites for those values. The game keeps the loaded movie for the whole session and creates the mission HUD
//! from it at every sortie, so:
//! - the first time the game opens the movie's file (boot), we keep that file object;
//! - when the game looks the movie up (every sortie), we read the rows, rewrite the kept file's bytes for them and
//!   drop the loaded movie, so the game loads it again;
//! - that load's file open gets the rewritten bytes, wrapped by the game's own memory file function.
//!
//! Nothing is written to disk. If the file the game opened isn't the prepared one (another mod's HUD movie won
//! over ours), the rows do nothing.

use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::hud_movie::{self, Look};
use crate::rows::{self, ROW_HUD};
use crate::{Hook, log, orig};

// The HUD row's choices, in percent. build.py adds as many materials to the regulation.
const OPACITIES: [u32; 11] = [0, 10, 20, 30, 40, 50, 60, 70, 80, 90, 100];

const HUD_MOVIE: &str = "01_000_FE";

// Menu movie lookup (scaleform system, out, menu table entry) -> out; loads the movie if it isn't cached.
type LookupFn = unsafe extern "system" fn(usize, usize, usize) -> usize;
// The game's Scaleform file opener (this, UTF-8 path, flags, mode) -> file.
type OpenFileFn = unsafe extern "system" fn(usize, *const u8, usize, usize) -> usize;
// GfxMemoryFile factory (name: Scaleform string, bytes, size) -> file with its own copy of the bytes.
type MemoryFileFn = unsafe extern "system" fn(usize, *const u8, usize) -> usize;
type AddRefFn = unsafe extern "system" fn(usize);

const MEMORY_FILE: usize = 0x14153cbc0;
const MEMORY_FILE_VTABLE: usize = 0x143ae83b8;
const FILE_NAME_OFFSET: usize = 0x10;
const FILE_DATA_OFFSET: usize = 0x18;
const FILE_SIZE_OFFSET: usize = 0x20;
const SCALEFORM_ADD_REF: usize = 0x141a58290;

// Menu table entry: the movie's name (wide).
const ENTRY_NAME_OFFSET: usize = 8;
// The scaleform system's movie cache: an MSVC std::map (head pointer here), key = name, value = movie wrapper.
const CACHE_MAP_HEAD_OFFSET: usize = 0x9c0;
const NODE_KEY_OFFSET: usize = 0x28;
const NODE_KEY_LENGTH_OFFSET: usize = 0x30;
const NODE_VALUE_OFFSET: usize = 0x248;
const WRAPPER_REFCOUNT_OFFSET: usize = 8;

static ORIG_LOOKUP: AtomicUsize = AtomicUsize::new(0);
static ORIG_OPEN_FILE: AtomicUsize = AtomicUsize::new(0);

// The file the game opened for the HUD movie at boot, kept alive: the base for every later load.
static BASE_FILE: AtomicUsize = AtomicUsize::new(0);

/// What the HUD movie's lookup wants done with the next file the game opens: that file is the movie's (the path
/// can't tell: mod loaders rename overridden files).
enum NextOpen {
    Nothing,
    KeepAsBase,
    Serve(Vec<u8>),
}
static NEXT_OPEN: Mutex<NextOpen> = Mutex::new(NextOpen::Nothing);

pub(crate) fn hooks() -> [Hook; 2] {
    [
        (0x141609040, [0x40, 0x55, 0x56, 0x57, 0x41, 0x54], lookup_impl as *const (), &ORIG_LOOKUP),
        (0x1415f40b0, [0x40, 0x55, 0x56, 0x57, 0x41, 0x56], open_file_impl as *const (), &ORIG_OPEN_FILE),
    ]
}

/// The game's own structures, read on its thread while it uses them: no need for the checked crate::read.
unsafe fn read<T: Copy>(addr: usize) -> T {
    unsafe { std::ptr::read_unaligned(addr as *const T) }
}

unsafe fn wide_text(chars: usize, max: usize) -> String {
    let mut len = 0;
    while len < max && unsafe { read::<u16>(chars + len * 2) } != 0 {
        len += 1;
    }
    String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(chars as *const u16, len) })
}

/// A memory file's bytes.
unsafe fn file_bytes(file: usize) -> &'static [u8] {
    let (data, size): (usize, u32) = unsafe { (read(file + FILE_DATA_OFFSET), read(file + FILE_SIZE_OFFSET)) };
    unsafe { std::slice::from_raw_parts(data as *const u8, size as usize) }
}

unsafe extern "system" fn lookup_impl(system: usize, out: usize, entry: usize) -> usize {
    let lookup: LookupFn = unsafe { orig(&ORIG_LOOKUP) };
    let name = unsafe { wide_text(read(entry + ENTRY_NAME_OFFSET), 64) };
    if !name.eq_ignore_ascii_case(HUD_MOVIE) {
        return unsafe { lookup(system, out, entry) };
    }
    let base = BASE_FILE.load(Ordering::Relaxed);
    let mut next_open = NextOpen::Nothing;
    if base == 0 {
        next_open = NextOpen::KeepAsBase;
    } else if let Some(look) = rows_look() {
        // The loaded movie is only dropped once there is one to serve in its place: the game can't reopen the file.
        if let Some((movie, colors)) = hud_movie::with_look(unsafe { file_bytes(base) }, look) {
            let Look { hud, warning, opacity } = look;
            log(&format!("hud: #{hud:06X}, warning #{warning:06X}, opacity {opacity}% ({colors} colors rewritten)"));
            next_open = NextOpen::Serve(movie);
            unsafe { release_cached_movie(system) };
        }
    }
    *NEXT_OPEN.lock().unwrap() = next_open;
    let result = unsafe { lookup(system, out, entry) };
    *NEXT_OPEN.lock().unwrap() = NextOpen::Nothing;
    result
}

unsafe extern "system" fn open_file_impl(this: usize, path: *const u8, flags: usize, mode: usize) -> usize {
    let open: OpenFileFn = unsafe { orig(&ORIG_OPEN_FILE) };
    // Taken, so only the first file opened during the lookup counts: the movie itself, not what it imports.
    let next_open = std::mem::replace(&mut *NEXT_OPEN.lock().unwrap(), NextOpen::Nothing);
    match next_open {
        NextOpen::Nothing => unsafe { open(this, path, flags, mode) },
        NextOpen::KeepAsBase => {
            let file = unsafe { open(this, path, flags, mode) };
            if file != 0 && unsafe { read::<usize>(file) } == MEMORY_FILE_VTABLE && hud_movie::is_prepared(unsafe { file_bytes(file) }) {
                let add_ref: AddRefFn = unsafe { std::mem::transmute(SCALEFORM_ADD_REF) };
                unsafe { add_ref(file) };
                BASE_FILE.store(file, Ordering::Relaxed);
                log("hud: kept the prepared HUD movie's file");
            } else {
                log("hud: the game didn't open the prepared HUD movie; the HUD rows won't apply");
            }
            file
        }
        NextOpen::Serve(movie) => {
            let memory_file: MemoryFileFn = unsafe { std::mem::transmute(MEMORY_FILE) };
            let name = BASE_FILE.load(Ordering::Relaxed) + FILE_NAME_OFFSET;
            unsafe { memory_file(name, movie.as_ptr(), movie.len()) }
        }
    }
}

/// Drops the cache's reference to the loaded HUD movie, so the lookup loads it again. Between missions nothing else
/// holds it (the mission HUD is gone), so it is freed; Scaleform would otherwise hand the same movie back.
unsafe fn release_cached_movie(system: usize) {
    let head: usize = unsafe { read(system + CACHE_MAP_HEAD_OFFSET) };
    if head == 0 {
        return;
    }
    // Map node: left +0, parent +8, right +0x10, is-nil +0x19. The head's parent is the root.
    let mut pending = vec![unsafe { read::<usize>(head + 8) }];
    let mut visited = 0;
    while let Some(node) = pending.pop() {
        if node == 0 || node == head || unsafe { read::<u8>(node + 0x19) } != 0 || visited > 1000 {
            continue;
        }
        visited += 1;
        let key_length: usize = unsafe { read(node + NODE_KEY_LENGTH_OFFSET) };
        let key = unsafe { wide_text(read(node + NODE_KEY_OFFSET), key_length.min(64)) };
        if !key.eq_ignore_ascii_case(HUD_MOVIE) {
            pending.push(unsafe { read(node) });
            pending.push(unsafe { read(node + 0x10) });
            continue;
        }
        let value = node + NODE_VALUE_OFFSET;
        let wrapper: usize = unsafe { read(value) };
        if wrapper == 0 {
            return;
        }
        unsafe { std::ptr::write_unaligned(value as *mut usize, 0) };
        let refcount = unsafe { &*((wrapper + WRAPPER_REFCOUNT_OFFSET) as *const std::sync::atomic::AtomicI32) };
        if refcount.fetch_sub(1, Ordering::SeqCst) == 1 {
            // vtable slot 0 destroys it
            let destroy: unsafe extern "system" fn(usize) = unsafe { std::mem::transmute(read::<usize>(read::<usize>(wrapper))) };
            unsafe { destroy(wrapper) };
        } else {
            log("hud: the old HUD movie is still in use; the new look may not show");
        }
        return;
    }
}

// ---- the rows -----------------------------------------------------------------------------------------------------

pub(crate) fn claim_rows() {
    let labels = OPACITIES.map(|percent| format!("Opacity {percent}%"));
    let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
    rows::claim(ROW_HUD, "HUD", Look::STOCK.hud);
    // Fully opaque, the last one, is the default.
    rows::set_choices(ROW_HUD, "Opacity", &labels, OPACITIES.len() as u32 - 1);
    rows::set_second_color(ROW_HUD, "Warning color", Look::STOCK.warning);
}

/// What the rows say right now. None while the player's data isn't loaded.
fn rows_look() -> Option<Look> {
    let hud = rows::color(ROW_HUD)?;
    let warning = rows::second_color(ROW_HUD)?;
    let opacity = *OPACITIES.get(rows::choice(ROW_HUD)? as usize)?;
    Some(Look { hud, warning, opacity })
}
