"""Logic of the Blender extension that does not need Blender.

Everything here works on plain Python values, NumPy arrays and the ``arbrume`` wheel, so it
is tested with pytest outside Blender. The modules that import ``bpy`` sit one level up and
only move data between these functions and Blender.
"""
