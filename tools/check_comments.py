"""Fail if any block of plain line comments is longer than 2 lines.

Rust: `//` blocks are checked; `///` and `//!` doc comments (item and module headers) are exempt.
Python: `#` blocks are checked; docstrings are exempt. A leading shebang/encoding line is ignored.
Usage: python tools/check_comments.py [root]   (exit code 1 on violations)
"""

import re
import sys
from pathlib import Path

MAX_LINES = 2
PATTERNS = {".rs": re.compile(r"^\s*//(?![/!])"), ".py": re.compile(r"^\s*#(?!!)")}
SKIP_DIRS = {"target", ".git", ".venv", "venv", "build", "dist", "__pycache__"}


def comment_blocks(lines, pattern):
    start = None
    for number, line in enumerate([*lines, ""], start=1):
        if pattern.match(line):
            start = start or number
        elif start:
            yield start, number - start
            start = None


def main(root):
    failures = 0
    for path in sorted(Path(root).rglob("*")):
        pattern = PATTERNS.get(path.suffix)
        if pattern is None or SKIP_DIRS.intersection(path.parts):
            continue
        lines = path.read_text(encoding="utf-8").splitlines()
        for start, length in comment_blocks(lines, pattern):
            if length > MAX_LINES:
                print(f"{path}:{start}: {length}-line comment (max {MAX_LINES}); use a doc header")
                failures += 1
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1] if len(sys.argv) > 1 else "."))
