"""Creates the sample logs of the guides in docs/guides (the logs of their screenshots).

    python scripts/gen_guide_logs.py [GUIDE ...]

GUIDE = failed-app-install, software-updates (default: all). The logs of a guide go to
testlogs/guides/<guide>/ (not in git) in the folder layout of a Windows PC (Windows/CCM/Logs, …).
All names, IDs and GUIDs in them are invented; the output is the same on every run.
"""
import os, runpy, sys

HERE = os.path.dirname(os.path.abspath(__file__))
GUIDES = {
    "failed-app-install": "failed_app_install.py",
    "software-updates": "software_updates.py",
}

names = sys.argv[1:] or list(GUIDES)
for name in names:
    if name not in GUIDES:
        sys.exit(f"unknown guide {name!r}, choose from: {', '.join(GUIDES)}")
for name in names:
    out = os.path.normpath(os.path.join(HERE, "..", "testlogs", "guides", name))
    print(f"== {name} -> {out}")
    sys.argv = [GUIDES[name], out]
    runpy.run_path(os.path.join(HERE, "guide_logs", GUIDES[name]), run_name="__main__")
