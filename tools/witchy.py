"""WitchyBND, as the build uses it: its unpacked folder names, and runs that check their outputs."""
import os
import re
import subprocess


def unpacked(path):
    """WitchyBND's folder naming, next to the file: regulation.bin -> regulation-bin, x.ffxbnd.dcx -> x-ffxbnd-dcx-wffxbnd
    (the same for matbinbnd), x.anibnd -> x-anibnd-wanibnd."""
    name = os.path.basename(path)
    kind = re.search(r"\.(ffxbnd|matbinbnd)\.dcx$|\.(anibnd)$", name)
    kind = kind and (kind.group(1) or kind.group(2))
    folder = name.replace(".", "-") + (f"-w{kind}" if kind else "")
    return os.path.join(os.path.dirname(path), folder)


def empty_like(path, stock, *subfolders):
    """An empty unpacked folder for a new archive at `path`, shaped like the unpacked archive `stock`: its WitchyBND
    manifest under our file name (WitchyBND packs whatever files the folder holds), and `subfolders`, returned."""
    folder = unpacked(path)
    os.makedirs(folder)
    manifest = next(f for f in os.listdir(stock) if f.startswith("_witchy"))
    text = open(os.path.join(stock, manifest), encoding="utf-8-sig").read()
    stock_name = re.search(r"<filename>(.*?)</filename>", text).group(1)
    open(os.path.join(folder, manifest), "w", encoding="utf-8-sig").write(text.replace(stock_name, os.path.basename(path)))
    paths = [os.path.join(folder, sub) for sub in subfolders]
    for sub in paths:
        os.makedirs(sub)
    return paths


class Witchy:
    def __init__(self, exe):
        self.exe = exe

    def __call__(self, inputs, outputs):
        """One WitchyBND run over all inputs (each launch is slow). It exits 0 even when it does nothing, so check that
        every expected output was (re)written."""
        before = {out: os.path.getmtime(out) if os.path.exists(out) else None for out in outputs}
        subprocess.run([self.exe, "-s", *inputs], check=True)
        stale = [out for out in outputs if not os.path.exists(out) or os.path.getmtime(out) == before[out]]
        assert not stale, f"WitchyBND didn't produce {stale}"
