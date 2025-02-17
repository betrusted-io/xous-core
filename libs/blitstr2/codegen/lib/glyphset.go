// Copyright (c) 2022 Sam Blenny
// SPDX-License-Identifier: Apache-2.0 OR MIT
//
package lib

import (
	"fmt"
)

// Holds an index list and rust source code for a font's worth of blit patterns
type GlyphSet struct {
	Glyphs        string
	Codepoints    string
	Widths        string
	GlyphsLen     int
	WidthsLen     int
	CodepointsLen int
	Index         CodepointIndex
}

// Make rust source code and an index list from a list of glyph blit patterns.
// The point of this is to prepare data in a way that's convenient for including
// in the context data used to render a .rs source code file template.
func NewGlyphSetFrom(pl []BlitPattern, fs FontSpec) GlyphSet {
	g := GlyphSet{"", "", "", 0, 0, 0, CodepointIndex{}}
	for _, p := range pl {
		g.Codepoints += fmt.Sprintf("0x%05X,\n", p.CS.Uint32FromHex())
		g.Glyphs += ConvertPatternToRust(p)
		// Update the block index with the correct offset (DATA[n]) for pattern header
		g.Insert(p.CS.Uint32FromHex(), g.GlyphsLen)
		g.GlyphsLen += len(p.Words)
		if fs.GlyphTrim == "proportional" || fs.GlyphTrim == "monospace" {
			g.Widths += fmt.Sprintf("%d,\n", p.Width)
			g.WidthsLen += 1
		}
		g.CodepointsLen += 1
	}
	return g
}

// Insert entry into index of (grapheme cluster hash, glyph blit pattern data offset)
func (g GlyphSet) Insert(codepoint uint32, dataOffset int) {
	g.Index = append(g.Index, codepointOffsetEntry{codepoint, dataOffset})
}
