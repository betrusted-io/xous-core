// Copyright (c) 2022 Sam Blenny
// SPDX-License-Identifier: Apache-2.0 OR MIT
//
package main

import (
	. "blitstr2/codegen/lib"
	"fmt"
	"image"
	"image/png"
	"io/ioutil"
	"os"
)

// Command line switch to confirm intent of writing output files
const confirm = "--write"

// Command line switch to enable debug output
const debug = "--debug"

// Change this to control the visibility of debug messages
var enableDebug = false

// Main: check for confirmation switch before writing files
func main() {
	if len(os.Args) == 2 && os.Args[1] == confirm {
		codegen()
	} else if len(os.Args) == 3 && os.Args[1] == confirm && os.Args[2] == debug {
		enableDebug = true
		codegen()
	} else if len(os.Args) == 3 && os.Args[1] == debug && os.Args[2] == confirm {
		enableDebug = true
		codegen()
	} else {
		usage()
	}
}

// Generate rust source code files for fonts
func codegen() {
	conf := NewConfig("config.json")
	fsList := conf.Fonts()
	for _, f := range fsList {
		// Find all the glyphs and pack them into a list of blit pattern objects
		pl := patternListFromSpriteSheet(f)
		// Make rust code for the blit pattern DATA array, plus an index list
		gs := NewGlyphSetFrom(pl, f)
		// Generate rust source code and write it to a file
		code := RenderFontFileTemplate(f, gs)
		fmt.Println("Writing to", f.RustOut)
		ioutil.WriteFile(f.RustOut, []byte(code), 0644)
	}
}

// Extract glyph sprites from a PNG grid and pack them into a list of blit pattern objects
func patternListFromSpriteSheet(fs FontSpec) []BlitPattern {
	// Read glyphs from png file
	img := readPNGFile(fs.Sprites)
	var patternList []BlitPattern
	for _, cs := range fs.CSList {
		blitPattern := NewBlitPattern(img, fs, cs, enableDebug)
		patternList = append(patternList, blitPattern)
	}
	return patternList
}

// Read the specified PNG file and convert its data into an image object
func readPNGFile(name string) image.Image {
	pngFile, err := os.Open(name)
	if err != nil {
		panic("unable to open png file")
	}
	img, err := png.Decode(pngFile)
	if err != nil {
		panic("unable to decode png file")
	}
	pngFile.Close()
	return img
}

// Print usage message
func usage() {
	conf := NewConfig("config.json")
	fsList := conf.Fonts()
	u := RenderUsageTemplate(confirm, debug, fsList)
	fmt.Println(u)
}
