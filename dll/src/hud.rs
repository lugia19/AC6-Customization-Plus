//! The HUD row in the Paint screen (NativePaint's row 1): a color and an opacity for the HUD. White and fully opaque
//! (the stock HUD) until picked.
//!
//! Each of the HUD movie's top-level elements gets a color transform (multiply) with the row's values. The game
//! keeps the loaded movie (menu\01_000_fe.gfx) for the whole session and creates the mission HUD from it at every
//! sortie, so:
//! - the first time the game opens the movie's file (boot), we keep that file object;
//! - when the game looks the movie up (every sortie) and the row has changed since the movie was loaded, we drop the
//!   loaded movie, so the game loads it again;
//! - that load's file open gets a new file from us: the kept file's bytes with the transforms added, wrapped by the
//!   game's own memory file function.
//!
//! Nothing is written to disk, and whatever HUD movie the game loaded at boot (stock or another mod's) is the base.

use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::time::Duration;

use crate::native_paint_client::{self, ROW_HUD};
use crate::{Hook, log, orig};

const POLL_INTERVAL: Duration = Duration::from_millis(200);

const DEFAULT_COLOR: u32 = 0xFFFFFF;
// The row's choices, in percent. build.py adds as many materials to the regulation.
const OPACITIES: [u32; 11] = [100, 90, 80, 70, 60, 50, 40, 30, 20, 10, 0];

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

const fn look(rgb: u32, opacity: u32) -> u64 {
    (rgb as u64) << 32 | opacity as u64
}
const STOCK_LOOK: u64 = look(DEFAULT_COLOR, 100);

// The row's values (poll thread), and the ones the loaded movie has.
static WANTED_LOOK: AtomicU64 = AtomicU64::new(STOCK_LOOK);
static LOADED_LOOK: AtomicU64 = AtomicU64::new(STOCK_LOOK);
// The file the game opened for the HUD movie at boot, kept alive: the base for every later load.
static BASE_FILE: AtomicUsize = AtomicUsize::new(0);
// Set while the game looks the HUD movie up: the next file it opens is the movie's.
static IN_HUD_LOOKUP: AtomicBool = AtomicBool::new(false);

pub(crate) fn hooks() -> [Hook; 2] {
    [
        (0x141609040, [0x40, 0x55, 0x56, 0x57, 0x41, 0x54], lookup_impl as *const (), &ORIG_LOOKUP),
        (0x1415f40b0, [0x40, 0x55, 0x56, 0x57, 0x41, 0x56], open_file_impl as *const (), &ORIG_OPEN_FILE),
    ]
}

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

unsafe extern "system" fn lookup_impl(system: usize, out: usize, entry: usize) -> usize {
    let lookup: LookupFn = unsafe { orig(&ORIG_LOOKUP) };
    let name = unsafe { wide_text(read(entry + ENTRY_NAME_OFFSET), 64) };
    if !name.eq_ignore_ascii_case(HUD_MOVIE) {
        return unsafe { lookup(system, out, entry) };
    }
    let wanted = WANTED_LOOK.load(Ordering::Relaxed);
    let have_base = BASE_FILE.load(Ordering::Relaxed) != 0;
    if have_base && LOADED_LOOK.load(Ordering::Relaxed) != wanted {
        log(&format!("hud: reloading the HUD movie for #{:06X}, opacity {}%", wanted >> 32, wanted as u32));
        unsafe { release_cached_movie(system) };
    }
    IN_HUD_LOOKUP.store(true, Ordering::Relaxed);
    let result = unsafe { lookup(system, out, entry) };
    IN_HUD_LOOKUP.store(false, Ordering::Relaxed);
    result
}

unsafe extern "system" fn open_file_impl(this: usize, path: *const u8, flags: usize, mode: usize) -> usize {
    let open: OpenFileFn = unsafe { orig(&ORIG_OPEN_FILE) };
    // Only the first file opened during the lookup: the movie itself, not what it imports.
    if !IN_HUD_LOOKUP.swap(false, Ordering::Relaxed) {
        return unsafe { open(this, path, flags, mode) };
    }
    let base = BASE_FILE.load(Ordering::Relaxed);
    if base == 0 {
        let file = unsafe { open(this, path, flags, mode) };
        if file != 0 && unsafe { read::<usize>(file) } == MEMORY_FILE_VTABLE {
            let add_ref: AddRefFn = unsafe { std::mem::transmute(SCALEFORM_ADD_REF) };
            unsafe { add_ref(file) };
            BASE_FILE.store(file, Ordering::Relaxed);
            log(&format!("hud: kept the HUD movie's file ({} bytes)", unsafe { read::<u32>(file + FILE_SIZE_OFFSET) }));
        } else {
            log(&format!("hud: the HUD movie's file isn't a memory file ({file:#x}); the HUD row won't apply"));
        }
        return file;
    }
    let wanted = WANTED_LOOK.load(Ordering::Relaxed);
    let stock = unsafe {
        std::slice::from_raw_parts(read::<usize>(base + FILE_DATA_OFFSET) as *const u8, read::<u32>(base + FILE_SIZE_OFFSET) as usize)
    };
    let Some(movie) = with_look(stock, (wanted >> 32) as u32, wanted as u32) else {
        log("hud: couldn't read the HUD movie's tags; the HUD row won't apply");
        return unsafe { open(this, path, flags, mode) };
    };
    let memory_file: MemoryFileFn = unsafe { std::mem::transmute(MEMORY_FILE) };
    let file = unsafe { memory_file(base + FILE_NAME_OFFSET, movie.as_ptr(), movie.len()) };
    if file != 0 {
        LOADED_LOOK.store(wanted, Ordering::Relaxed);
    }
    file
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

// ---- the movie's tags -------------------------------------------------------------------------------------------

const TAG_END: u16 = 0;
const TAG_PLACE_OBJECT2: u16 = 26;
const TAG_PLACE_OBJECT3: u16 = 70;
const PLACE_HAS_CHARACTER: u8 = 0x02;
const PLACE_HAS_MATRIX: u8 = 0x04;
const PLACE_HAS_COLOR_TRANSFORM: u8 = 0x08;
// PlaceObject3's second flags byte: a class name follows the depth.
const PLACE3_HAS_CLASS_NAME: u8 = 0x08;
const PLACE3_HAS_IMAGE: u8 = 0x10;

/// Reads `count` bits (most significant first) at bit `pos`.
fn bits(data: &[u8], pos: usize, count: usize) -> Option<usize> {
    let mut value = 0;
    for i in pos..pos + count {
        value = value << 1 | (*data.get(i / 8)? >> (7 - i % 8) & 1) as usize;
    }
    Some(value)
}

/// Byte length of the MATRIX record at the start of `data`: optional scale and rotate pairs, then the translation,
/// each a 5-bit field width and two fields.
fn matrix_length(data: &[u8]) -> Option<usize> {
    let mut pos = 0;
    for _ in 0..2 {
        let present = bits(data, pos, 1)?;
        pos += 1;
        if present == 1 {
            pos += 5 + 2 * bits(data, pos, 5)?;
        }
    }
    pos += 5 + 2 * bits(data, pos, 5)?;
    Some(pos.div_ceil(8))
}

/// CXFORMWITHALPHA record, multiply terms only (256 = 1.0): no add terms, multiply terms, 10-bit fields.
fn color_transform(rgb: u32, opacity: u32) -> [u8; 6] {
    let terms = [rgb >> 16 & 0xFF, rgb >> 8 & 0xFF, rgb & 0xFF].map(|c| (c * 256 + 127) / 255);
    let mut record: u64 = 0b01_1010;
    for term in terms.into_iter().chain([opacity * 256 / 100]) {
        record = record << 10 | term as u64;
    }
    let bytes = (record << 2).to_be_bytes(); // 46 bits, padded to 6 bytes
    [bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7]]
}

/// The movie with a color transform on every top-level placement that has none. None if its tags don't parse.
fn with_look(movie: &[u8], rgb: u32, opacity: u32) -> Option<Vec<u8>> {
    let transform = color_transform(rgb, opacity);
    // Header: signature and version, file length (u32 at 4), the stage RECT (5-bit field width, 4 fields), frame rate
    // and count.
    let rect_bits = (*movie.get(8)? >> 3) as usize;
    let mut pos = 8 + (5 + 4 * rect_bits).div_ceil(8) + 4;
    let mut out = movie.get(..pos)?.to_vec();
    let mut changed = 0;
    loop {
        // Tag header: type in the top 10 bits, length in the low 6; 0x3f = a u32 length follows.
        let code = u16::from_le_bytes(movie.get(pos..pos + 2)?.try_into().ok()?);
        let (tag_type, mut length, mut header) = (code >> 6, (code & 0x3f) as usize, 2);
        if length == 0x3f {
            length = u32::from_le_bytes(movie.get(pos + 2..pos + 6)?.try_into().ok()?) as usize;
            header = 6;
        }
        let body = movie.get(pos + header..pos + header + length)?;
        let insert_at = match tag_type {
            TAG_PLACE_OBJECT2 | TAG_PLACE_OBJECT3 => transform_position(tag_type, body),
            _ => None,
        };
        match insert_at {
            Some(at) => {
                let length = length + transform.len();
                if length < 0x3f {
                    out.extend(((tag_type << 6) | length as u16).to_le_bytes());
                } else {
                    out.extend(((tag_type << 6) | 0x3f).to_le_bytes());
                    out.extend((length as u32).to_le_bytes());
                }
                out.push(body[0] | PLACE_HAS_COLOR_TRANSFORM);
                out.extend(&body[1..at]);
                out.extend(transform);
                out.extend(&body[at..]);
                changed += 1;
            }
            None => out.extend(&movie[pos..pos + header + length]),
        }
        pos += header + length;
        if tag_type == TAG_END {
            break;
        }
    }
    out.extend(&movie[pos..]);
    let size = (out.len() as u32).to_le_bytes();
    out[4..8].copy_from_slice(&size);
    (changed > 0).then_some(out)
}

/// Where a placement's color transform goes (after its matrix), if it places a character and has none yet.
fn transform_position(tag_type: u16, body: &[u8]) -> Option<usize> {
    let flags = *body.first()?;
    if flags & PLACE_HAS_CHARACTER == 0 || flags & PLACE_HAS_COLOR_TRANSFORM != 0 {
        return None;
    }
    let mut pos = 1;
    if tag_type == TAG_PLACE_OBJECT3 {
        let flags2 = *body.get(1)?;
        if flags2 & (PLACE3_HAS_CLASS_NAME | PLACE3_HAS_IMAGE) != 0 {
            return None;
        }
        pos += 1;
    }
    pos += 4; // depth, character
    if flags & PLACE_HAS_MATRIX != 0 {
        pos += matrix_length(body.get(pos..)?)?;
    }
    (pos <= body.len()).then_some(pos)
}

// ---- the row ------------------------------------------------------------------------------------------------------

fn poll_loop() {
    let labels = OPACITIES.map(|percent| format!("Opacity {percent}%"));
    let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
    let mut claimed = false;
    loop {
        std::thread::sleep(POLL_INTERVAL);
        if !claimed {
            claimed = native_paint_client::claim_row(ROW_HUD, "HUD", DEFAULT_COLOR);
            if claimed {
                let choices = native_paint_client::set_choices(ROW_HUD, &labels, 0);
                log(&format!("hud: HUD row claimed (choices: {choices})"));
            }
            continue;
        }
        let (Some(rgb), Some(choice)) = (native_paint_client::color(ROW_HUD), native_paint_client::choice(ROW_HUD))
        else {
            continue;
        };
        let opacity = OPACITIES.get(choice as usize).copied().unwrap_or(100);
        if WANTED_LOOK.swap(look(rgb, opacity), Ordering::Relaxed) != look(rgb, opacity) {
            log(&format!("hud: #{rgb:06X}, opacity {opacity}% (from the next sortie)"));
        }
    }
}

pub(crate) fn start() {
    std::thread::spawn(poll_loop);
}
