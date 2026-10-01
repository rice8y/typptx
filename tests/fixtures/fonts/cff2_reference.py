"""Generate independent CFF2 fixture metrics; requires fontTools 4.63.0."""

import json
from pathlib import Path

from fontTools.pens.boundsPen import BoundsPen
from fontTools.ttLib import TTFont
from fontTools.varLib.instancer import instantiateVariableFont

root = Path(__file__).resolve().parent
references = []
for weight, size in [(300, 12), (700, 12), (300, 48)]:
    font = TTFont(root / "SourceSerif4Variable-HelloWorld.otf")
    font = instantiateVariableFont(font, {"wght": weight, "opsz": size})
    glyphs = font.getGlyphSet()
    cmap = font.getBestCmap()
    metrics = {}
    for character in sorted(set("HelloWorld")):
        glyph = glyphs[cmap[ord(character)]]
        pen = BoundsPen(glyphs)
        glyph.draw(pen)
        metrics[character] = {"advance": glyph.width, "bounds": pen.bounds}
    references.append({"weight": weight, "size": size, "glyphs": metrics})

(root / "SourceSerif4-reference.json").write_text(
    json.dumps(references, indent=2) + "\n", encoding="utf-8"
)
