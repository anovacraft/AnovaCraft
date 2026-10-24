"""Build AnovaCraftMarks.ttf: combining marks that sit over the preceding
character without OpenType mark positioning (which egui does not apply).

egui draws each character at its own advance, so a combining mark must carry
its own offset back over the previous glyph. These marks are Inter's macron
and circumflex (SIL Open Font License 1.1, no Reserved Font Name), moved up to
capital height and left by half a typical capital's width, with zero advance.

Run: python3 build_marks.py (needs fontTools).
"""
from fontTools.fontBuilder import FontBuilder
from fontTools.pens.cu2quPen import Cu2QuPen
from fontTools.pens.transformPen import TransformPen
from fontTools.pens.ttGlyphPen import TTGlyphPen
from fontTools.ttLib import TTFont

inter = TTFont("Inter-Regular.otf")
gs = inter.getGlyphSet()
cmap = inter.getBestCmap()
hmtx = inter["hmtx"]
X = hmtx[cmap[ord("X")]][0]  # capital width the marks centre over
LIFT = 372  # lowercase mark height → capital mark height (Inter's Ā)


def moved(name, centre_on):
    """Inter glyph `name` centred at −centre_on/2 and lifted to capital height."""
    from fontTools.pens.boundsPen import BoundsPen

    bp = BoundsPen(gs)
    gs[name].draw(bp)
    x0, _, x1, _ = bp.bounds
    dx = -centre_on / 2 - (x0 + x1) / 2
    pen = TTGlyphPen(None)
    gs[name].draw(TransformPen(Cu2QuPen(pen, 1.0, reverse_direction=True), (1, 0, 0, 1, dx, LIFT)))
    return pen.glyph()


glyphs = {".notdef": TTGlyphPen(None).glyph(), "macroncomb": moved("uni0304", X), "circumflexcomb": moved("uni0302", X)}
fb = FontBuilder(inter["head"].unitsPerEm, isTTF=True)
fb.setupGlyphOrder(list(glyphs))
fb.setupCharacterMap({0x0304: "macroncomb", 0x0305: "macroncomb", 0x0302: "circumflexcomb"})
fb.setupGlyf(glyphs)
fb.setupHorizontalMetrics({g: (0, 0) for g in glyphs})
hhea = inter["hhea"]
fb.setupHorizontalHeader(ascent=hhea.ascent, descent=hhea.descent)
fb.setupNameTable(
    {
        "familyName": "AnovaCraft Marks",
        "styleName": "Regular",
        "copyright": "Copyright 2016 The Inter Project Authors; marks repositioned for AnovaCraft",
        "licenseDescription": "This Font Software is licensed under the SIL Open Font License, Version 1.1.",
        "licenseInfoURL": "https://openfontlicense.org",
    }
)
fb.setupOS2(sTypoAscender=hhea.ascent, sTypoDescender=hhea.descent, usWinAscent=hhea.ascent, usWinDescent=-hhea.descent)
fb.setupPost()
fb.save("AnovaCraftMarks.ttf")
print("wrote AnovaCraftMarks.ttf")
