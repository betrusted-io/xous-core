// Copyright (c) 2022 Sam Blenny
// SPDX-License-Identifier: Apache-2.0 OR MIT
//
package lib

import (
	"sort"
)

// Index for codepoints
type CodepointIndex []codepointOffsetEntry

// An index entry for translating from grapheme cluster to blit pattern
type codepointOffsetEntry struct {
	Codepoint  uint32
	DataOffset int
}

// Insert an index entry for (codepoint, glyph blit pattern data offset).
// Maintain sort order according to codepoints.
func (c CodepointIndex) Insert(codepoint uint32, dataOffset int) CodepointIndex {
	indexEntry := codepointOffsetEntry{
		codepoint,
		dataOffset,
	}
	c = append(c, indexEntry)
	// Sort by codepoint
	sort.Slice(c, func(i, j int) bool { return c[i].Codepoint < c[j].Codepoint })
	return c
}
