"""Prepare a font for bundling: drop its private-use code points (and the
glyphs only they reach), keeping all else. With --tabular, the digits are
the font's tabular ones (its `tnum` feature).

Usage: prepare_font.py SOURCE DESTINATION [--tabular]
"""
import sys
from fontTools.ttLib import TTFont
from fontTools import subset
src, dst = sys.argv[1], sys.argv[2]
font = TTFont(src, recalcTimestamp=False)
if "--tabular" in sys.argv:
    gsub = font["GSUB"].table
    lookups = {i for fr in gsub.FeatureList.FeatureRecord if fr.FeatureTag == "tnum"
               for i in fr.Feature.LookupListIndex}
    tnum = {}
    for i in lookups:
        lookup = gsub.LookupList.Lookup[i]
        for st in lookup.SubTable:
            st = st.ExtSubTable if lookup.LookupType == 7 else st
            tnum.update(getattr(st, "mapping", {}))
    for table in font["cmap"].tables:
        for code in range(ord("0"), ord("9") + 1):
            if code in table.cmap and table.cmap[code] in tnum:
                table.cmap[code] = tnum[table.cmap[code]]
keep = [c for c in font.getBestCmap() if not (0xE000 <= c <= 0xF8FF or 0xF0000 <= c <= 0x10FFFF)]
opts = subset.Options()
opts.layout_features = ["*"]
opts.name_IDs = ["*"]
opts.name_languages = ["*"]
opts.name_legacy = True
opts.glyph_names = False
opts.notdef_outline = True
opts.hinting = True
opts.legacy_kern = True
opts.symbol_cmap = True
opts.prune_unicode_ranges = True
opts.recalc_bounds = False
opts.drop_tables = []
s = subset.Subsetter(opts)
s.populate(unicodes=keep)
s.subset(font)
font.save(dst)
