# Sphinx configuration for the Arbrume developer docs.
#
# With the Vlam3d shared setup (vlam3d-doc-utils, a sibling clone that is not public) the
# site gets the archilab look and sidebar, and paper folder names link to the paper
# library. Without it, the docs build with plain Furo and the same content.

project = "Arbrume"
author = "Les Logiciels VLAM3D inc."
copyright = "2026, Les Logiciels VLAM3D inc."
language = "en"

extensions = ["myst_parser"]
source_suffix = {".md": "markdown"}
myst_enable_extensions = ["dollarmath", "amsmath", "colon_fence", "deflist", "tasklist"]
myst_heading_anchors = 3
exclude_patterns = ["_build"]

html_theme = "furo"
html_title = "Arbrume"

try:
    from vlam3d_doc_utils import configure
except ImportError:
    pass
else:
    configure(globals(), "arbrume", intersphinx=("papers",), paper_links=True)
