# The HUD row

How the HUD row recolors the in-mission HUD (`dll/src/hud.rs`, `dll/src/hud_movie.rs`, `tools/prepare_hud.py`;
Armored Core VI 1.09.1).

The game loads the HUD movie (`menu/01_000_fe.gfx`) once and builds each mission's HUD from it. The
package ships a prepared copy of it. Every time a sortie asks for the movie, the DLL reads the HUD rows,
drops the loaded movie and hands the game's loader a copy of the file it opened at boot, rewritten for them
(`dll/src/hud.rs`). Nothing is written to disk and nothing is remembered between sorties.

The movie holds about 2,400 colors: shape fills and outlines, text colors and color transforms. The build
(`tools/prepare_hud.py`) finds them all and appends a table of where they are to the movie, past the length the
game reads; it also gives each of the movie's 54 visible top-level elements a neutral color transform. The DLL
(`dll/src/hud_movie.rs`) then only runs each color in the table through one rule and writes the opacity into
those transforms' alpha. The rule: the movie's colors fall in two groups. Pale and grey ones are the HUD's base
color (`#DFFCFF` and its shades): they become the HUD color at the same brightness. Saturated ones are state
colors (the stagger bar's orange and red, reload and overheat, low AP, boss HP green, condition gauge
purple...): their hue is rotated, and their saturation and brightness scaled, by what takes pure red to the
warning color.
