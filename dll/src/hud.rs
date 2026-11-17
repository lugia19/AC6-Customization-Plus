//! The HUD row in the Paint screen (NativePaint's row 1): a color and an opacity for the HUD. White and fully opaque
//! (the stock HUD) until picked.
//!
//! So far it only claims the row and logs what the player picks; nothing is applied to the HUD yet.

use std::time::Duration;

use crate::log;
use crate::native_paint_client::{self, ROW_HUD};

const POLL_INTERVAL: Duration = Duration::from_millis(200);

const DEFAULT_COLOR: u32 = 0xFFFFFF;
// The row's choices, in percent. build.py adds as many materials to the regulation.
const OPACITIES: [u32; 11] = [100, 90, 80, 70, 60, 50, 40, 30, 20, 10, 0];

fn poll_loop() {
    let labels = OPACITIES.map(|percent| format!("Opacity {percent}%"));
    let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
    let mut claimed = false;
    let mut last = None;
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
        if last != Some((rgb, choice)) {
            last = Some((rgb, choice));
            let opacity = OPACITIES.get(choice as usize).copied().unwrap_or(100);
            log(&format!("hud: #{rgb:06X}, opacity {opacity}%"));
        }
    }
}

pub(crate) fn start() {
    std::thread::spawn(poll_loop);
}
