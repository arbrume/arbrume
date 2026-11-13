"""Makes the ``bpy``-free ``logic`` package of the extension importable by the tests."""

import sys
from pathlib import Path

EXTENSION = Path(__file__).resolve().parents[1] / "arbrume"
sys.path.insert(0, str(EXTENSION))
