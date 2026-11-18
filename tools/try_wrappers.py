"""Development helper: rebuilds only the flame wrappers (fxr/build_fxrs.mjs, e.g. after changing the ALLMIND fan's
layout) and reloads them in the running game with an FXR reloader, so a change shows on the next flame that spawns
without a restart. Needs a finished build.py run (it uses build/'s unpacked effects and flame rows).

    python tools/try_wrappers.py --reloader "<...>/fxr-reloader-cli.exe"

The reloaded wrappers only live in the game's memory: run build.py to put them in the package.
"""
import argparse
import glob
import os
import shutil
import subprocess

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
WORK = os.path.join(ROOT, "build")


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--reloader", required=True, help="fxr-reloader-cli.exe")
    args = parser.parse_args()

    effect_dir = os.path.join(WORK, "sfxbnd_commoneffects-ffxbnd-dcx-wffxbnd", "effect")
    wrappers = os.path.join(WORK, "wrappers")
    shutil.rmtree(wrappers, ignore_errors=True)
    os.makedirs(wrappers)
    subprocess.run(["node", "build_fxrs.mjs", effect_dir, os.path.join(WORK, "id_map.json"),
                    os.path.join(WORK, "flame_rows.json"),
                    os.path.join(WORK, "sfxbnd_m30-ffxbnd-dcx-wffxbnd", "effect"), wrappers],
                   cwd=os.path.join(ROOT, "fxr"), check=True, shell=os.name == "nt")

    tasks = subprocess.run(["tasklist", "/FI", "IMAGENAME eq armoredcore6.exe", "/FO", "CSV", "/NH"],
                           capture_output=True, text=True).stdout.splitlines()
    rows = [line.split('","') for line in tasks if line.startswith('"armoredcore6')]
    assert rows, "the game isn't running"
    # From its own folder: it loads its agent DLL from there.
    reloader = os.path.abspath(args.reloader)
    subprocess.run([reloader, "--process-id", rows[0][1], "--fxrs", *sorted(glob.glob(os.path.join(wrappers, "*.fxr")))],
                   cwd=os.path.dirname(reloader), check=True)
    print("wrappers reloaded")


if __name__ == "__main__":
    main()
