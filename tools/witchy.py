"""WitchyBND, as the build uses it: its unpacked folder names, and runs that check their outputs."""
import os
import re
import subprocess


def unpacked(path):
    """WitchyBND's folder naming, next to the file: regulation.bin -> regulation-bin, x.ffxbnd.dcx -> x-ffxbnd-dcx-wffxbnd
    (the same for matbinbnd)."""
    name = os.path.basename(path)
    kind = re.search(r"\.(ffxbnd|matbinbnd)\.dcx$", name)
    folder = name.replace(".", "-") + (f"-w{kind.group(1)}" if kind else "")
    return os.path.join(os.path.dirname(path), folder)


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
