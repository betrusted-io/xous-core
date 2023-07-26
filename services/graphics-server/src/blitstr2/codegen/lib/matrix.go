// Copyright (c) 2022 Sam Blenny
// SPDX-License-Identifier: Apache-2.0 OR MIT
//
package lib

import (
	"fmt"
)

// Holds a matrix of pixel values
type Matrix [][]int

// Holds one row of pixel values from a matrix
type MatrixRow []int

// Return pixel matrix as 16*16px sprite pattern packed into a [u32] array.
// pat[0:7]: 1-bit pixels packed into u32 words
//
// Pixel bit values are intended as a background/foreground mask for use with
// XOR blit. Color palette is not set. Rather, palette depends on contents of
// whatever bitmap the blit pattern gets XOR'ed with.
//
// Meaning of pixel bit values in blit pattern:
// - bit=0: keep color of pixel from background bitmap
// - bit=1: invert color of pixel from background bitmap
//
// Pixel packing happens in row-major order (first left to right, then top to
// bottom) with the glyph's top-left pixel placed in the least significant bit
// of the first pixel word.
func (m Matrix) convertToPattern() []uint32 {
	// Pack trimmed pattern into a byte array
	wide := len(m[0])
	high := len(m)
	pattern := []uint32{}
	bufWord := uint32(0)
	bits := 0
	for y := 0; y < high; y++ {
		for x := 0; x < wide; x++ {
			shift := ((y % 2) * 16) + x
			bufWord |= uint32(m[y][x]) << shift
			bits += 1
			if bits == 32 {
				pattern = append(pattern, bufWord)
				bufWord = 0
				bits = 0
			}
		}
	}
	if bits > 0 {
		finalShift := 32 - bits
		bufWord <<= finalShift
		pattern = append(pattern, bufWord)
	}
	return pattern
}

func (m Matrix) convertToPattern32() []uint32 {
	// Pack trimmed pattern into a byte array
	wide := len(m[0])
	high := len(m)
	pattern := []uint32{}
	bufWord := uint32(0)
	bits := 0
	for y := 0; y < high; y++ {
		for x := 0; x < wide; x++ {
			bufWord |= uint32(m[y][x]) << x
			bits += 1
			if bits == 32 {
				pattern = append(pattern, bufWord)
				bufWord = 0
				bits = 0
			}
		}
	}
	if bits > 0 {
		finalShift := 32 - bits
		bufWord <<= finalShift
		pattern = append(pattern, bufWord)
	}
	return pattern
}

// Trim pixel matrix to remove whitespace around the glyph. Return the trimmed
// matrix and the y-offset (pixels of top whitespace that were trimmed).
func (m Matrix) Trim(font FontSpec, row int, col int) Matrix {
	// Trim left whitespace
	trblTrimLimit := font.TrimLimits(row, col)
	m = m.transpose()
	m = m.trimLeadingEmptyRows(trblTrimLimit[3])
	// Trim right whitespace
	m = m.reverseRows()
	m = m.trimLeadingEmptyRows(trblTrimLimit[1])
	m = m.reverseRows()
	m = m.transpose()
	// Don't trim top whitespace
	// Don't trim bottom whitespace
	return m
}

// Pad a matrix to 16x16, adding padding to the right and bottom
func (m Matrix) padTo16x16() Matrix {
	// Make an empty 16x16 destination matrix
	dest := Matrix{}
	for y := 0; y < 16; y++ {
		row := MatrixRow{}
		for x := 0; x < 16; x++ {
			row = append(row, 0)
		}
		dest = append(dest, row)
	}
	// Copy pixels from source matrix to top-left of destination matrix
	wide := len(m[0])
	high := len(m)
	for y := 0; y < high; y++ {
		for x := 0; x < wide; x++ {
			dest[y][x] = m[y][x]
		}
	}
	return dest
}

// Pad a matrix to 32x32, adding padding to the right and bottom
func (m Matrix) padTo32x32() Matrix {
	// Make an empty 32x32 destination matrix
	dest := Matrix{}
	for y := 0; y < 32; y++ {
		row := MatrixRow{}
		for x := 0; x < 32; x++ {
			row = append(row, 0)
		}
		dest = append(dest, row)
	}
	// Copy pixels from source matrix to top-left of destination matrix
	wide := len(m[0])
	high := len(m)
	for y := 0; y < high; y++ {
		for x := 0; x < wide; x++ {
			dest[y][x] = m[y][x]
		}
	}
	return dest
}

// Transpose a matrix (flip around diagonal)
func (m Matrix) transpose() Matrix {
	if len(m) < 1 {
		return m
	}
	w := len(m[0])
	h := len(m)
	var transposed Matrix
	for col := 0; col < w; col++ {
		var trRow []int
		for row := 0; row < h; row++ {
			trRow = append(trRow, m[row][col])
		}
		transposed = append(transposed, trRow)
	}
	return transposed
}

// Reverse the order of rows in a matrix
func (m Matrix) reverseRows() Matrix {
	var reversed Matrix
	for i := len(m) - 1; i >= 0; i-- {
		reversed = append(reversed, m[i])
	}
	return reversed
}

// Trim whitespace rows from top of matrix
func (m Matrix) trimLeadingEmptyRows(limit int) Matrix {
	if len(m) < 1 {
		return m
	}
	for i := 0; i < limit; i++ {
		sum := 0
		for _, n := range m[0] {
			sum += n
		}
		if len(m) > 0 && sum == 0 {
			m = m[1:]
		} else {
			break
		}
	}
	return m
}

// Dump an ASCII art approximation of the blit pattern to stdout. This can help
// with troubleshooting character map setup when adding a new font.
func (m Matrix) Debug(cs CharSpec, enable bool) {
	if enable {
		cp := cs.Hex
		fmt.Printf("%X:\n", cp)
		fmt.Println(m.convertToText())
	}
}

// Return glyph as text with one ASCII char per pixel
func (m Matrix) convertToText() string {
	var ascii string
	for _, row := range m {
		for _, px := range row {
			if px == 1 {
				ascii += "█" // "\u2588"
			} else {
				ascii += "."
			}
		}
		ascii += "\n"
	}
	return ascii
}
