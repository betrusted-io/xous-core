// Copyright (c) 2022 Sam Blenny
// SPDX-License-Identifier: Apache-2.0 OR MIT
//
package lib

import (
	"fmt"
	"io/ioutil"
	"strconv"
	"strings"
)

// Holds mappings from extended grapheme clusters to sprite sheet glyph grid coordinates
type CharSpec struct {
	Hex string
	Row int
	Col int
}

// Parse a hex-codepoint format grapheme cluster into a uint32
// For example, "1f3c4" -> 0x1f3c4
func (cs CharSpec) Uint32FromHex() uint32 {
	base := 16
	bits := 32
	n, err := strconv.ParseUint(cs.Hex, base, bits)
	if err != nil {
		panic(fmt.Errorf("unexpected value for hex: %q", cs.Hex))
	}
	return uint32(n)
}

// Return mapping of hex-codepoint format grapheme clusters to grid coordinates
// in a glyph sprite sheet for the emoji font
func CJKMap(columns int, inputFile string) []CharSpec {
	text, err := ioutil.ReadFile(inputFile)
	if err != nil {
		panic(err)
	}
	// Start at top left corner of the sprite sheet glyph grid
	row := 0
	col := 0
	// Parse hex format codepoint lines that should look like
	// "1f4aa\n" "1f4e1\n", etc. Comments starting with "#" are
	// possible. Order of codepoint lines in the file should match a
	// row-major order traversal of the glyph grid.
	csList := []CharSpec{}
	for _, line := range strings.Split(string(text), "\n") {
		// Trim comments and leading/trailing whitespace
		txt := strings.TrimSpace(strings.SplitN(line, "#", 2)[0])
		if len(txt) > 0 {
			// Add a CharSpec for this codepoint
			csList = append(csList, CharSpec{txt, row, col})
			// Advance to next glyph position by row-major order
			col += 1
			if col == columns {
				row += 1
				col = 0
			}
		}
		// Skip blank lines and comments
	}
	return csList
}
