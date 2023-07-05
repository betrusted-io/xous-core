// Copyright (c) 2022 Sam Blenny
// SPDX-License-Identifier: Apache-2.0 OR MIT
//
package lib

import (
	"fmt"
	"image"
	"strings"
)

// Holds packed XOR mask values of a blit pattern for character's glyph.
//
// Pixel order: row-major order traversal of px matrix; top-left pixel goes in
//              least significant bit of of .Words[1]
// Mask values: bit=1 means foreground, bit=0 means background
//
// Patterns that need padding because their size is not a multiple of 32 bits
// (width*height % 32 != 0) get padded with zeros in the least significant bits
// of the last word.
type BlitPattern struct {
	Words []uint32
	Width uint8
	CS    CharSpec
}

// Extract matrix of pixels from an image containing grid of glyphs
// - img: image.Image from png file containing glyph grid
// - font: Glyph sheet specs (glyph size, border/gutter, etc)
// - cs: Character specs (source row and column in glyph grid)
func NewBlitPattern(img image.Image, font FontSpec, cs CharSpec, dbg bool) BlitPattern {
	row := cs.Row
	col := cs.Col
	imgRect := img.Bounds()
	rows := (imgRect.Max.Y - font.Border) / (font.Size + font.Gutter)
	if row < 0 || row >= rows || col < 0 || col >= font.Cols {
		panic("row or column out of range")
	}
	// Get pixels for grid cell, converting from RGBA to 1-bit
	gridSize := font.Size + font.Gutter
	border := font.Border
	pxMatrix := Matrix{}
	for y := border + (row * gridSize); y < (row+1)*gridSize; y++ {
		var row MatrixRow
		for x := border + (col * gridSize); x < (col+1)*gridSize; x++ {
			r, _, _, _ := img.At(x, y).RGBA()
			//fmt.Println(r, g, b, a)
			if r == 0 {
				row = append(row, 1)
			} else {
				row = append(row, 0)
			}
		}
		pxMatrix = append(pxMatrix, row)
	}
	pxMatrix = pxMatrix.Trim(font, row, col)
	width := uint8(len(pxMatrix[0]))
	if font.Size <= 16 {
		pxMatrix = pxMatrix.padTo16x16()
	} else {
		pxMatrix = pxMatrix.padTo32x32()
	}
	pxMatrix.Debug(cs, dbg)
	if font.Size <= 16 {
		patternBytes := pxMatrix.convertToPattern()
		return BlitPattern{patternBytes, width, cs}
	} else {
		patternBytes := pxMatrix.convertToPattern32()
		return BlitPattern{patternBytes, width, cs}
	}
}

// Convert blit pattern to rust source code for part of an array of bytes
func ConvertPatternToRust(pattern BlitPattern) string {
	patternStr := ""
	wordsPerRow := uint32(8)
	ceilRow := uint32(len(pattern.Words)) / wordsPerRow
	if uint32(len(pattern.Words))%wordsPerRow > 0 {
		ceilRow += 1
	}
	for i := uint32(0); i < ceilRow; i++ {
		start := i * wordsPerRow
		end := min(uint32(len(pattern.Words)), (i+1)*wordsPerRow)
		line := pattern.Words[start:end]
		var s []string
		for _, word := range line {
			s = append(s, fmt.Sprintf("0x%08x", word))
		}
		patternStr += strings.Join(s, ", ") + ","
	}
	patternStr += "\n"
	return patternStr
}

// Return lowest value among two integers
func min(a uint32, b uint32) uint32 {
	if b > a {
		return a
	}
	return b
}
