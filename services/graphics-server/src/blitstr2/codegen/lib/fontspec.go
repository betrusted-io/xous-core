// Copyright (c) 2022 Sam Blenny
// SPDX-License-Identifier: Apache-2.0 OR MIT
//
package lib

// Holds description of sprite sheet and character map for generating a font
type FontSpec struct {
	Name      string     // Name of font
	Sprites   string     // Which file holds the sprite sheet image with the grid of glyphs?
	Size      int        // How many pixels on a side is each glyph (precondition: square glyphs)
	Cols      int        // How many glyphs wide is the grid?
	Gutter    int        // How many px between glyphs?
	Border    int        // How many px wide are top and left borders?
	Legal     string     // Credits or license notices to included in .rs font file comments
	CSList    []CharSpec // Map of grapheme clusters to glyph grid coordinates
	RustOut   string     // Where should the generated rust source code go?
	GlyphTrim string     // How should bitmap glyphs be trimmed (proportional or CJK)?
	LoaderOut string     // Path to the split of the glyph data into the loader, to reduce the RAM load
	Small     bool       // Is this font part of the "small" configuration set
}

// Look up trim limits based on row & column in glyph grid
func (f FontSpec) TrimLimits(row int, col int) [4]int {
	if f.Name == "Bold" || f.Name == "Regular" || f.Name == "Small" || f.Name == "Tall" {
		// Space gets 4px width and 2px height
		if col == 2 && row == 0 {
			lr := (f.Size / 2) - 2
			tb := (f.Size / 2) - 1
			return [4]int{tb, lr, tb, lr}
		}
	} else if f.GlyphTrim == "CJK" {
		// No trim for CJK
		return [4]int{0, 0, 0, 0}
	} else if f.GlyphTrim == "monospace" {
		// Monospace gets 4 left, 4 right, none top & bottom
		return [4]int{0, 4, 0, 4}
	}
	// Everything else gets max trim
	return [4]int{f.Size, f.Size, f.Size, f.Size}
}
