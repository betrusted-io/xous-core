// Copyright (c) 2022 Sam Blenny
// SPDX-License-Identifier: Apache-2.0 OR MIT
//
package main

import (
	"blitstr2/codegen/lib"
	. "blitstr2/codegen/lib"
	"fmt"
	"image"
	"image/png"
	"io/ioutil"
	"os"
	"strings"
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
	var fdir []FontSummary
	var offsets []FontMap
	var small_offsets []FontMap
	var cur_address int = 0
	var small_cur_address int = 0
	for _, f := range fsList {
		// Find all the glyphs and pack them into a list of blit pattern objects
		pl := patternListFromSpriteSheet(f)
		// Make rust code for the blit pattern DATA array, plus an index list
		gs := NewGlyphSetFrom(pl, f)
		// Generate rust source code and write it to a file
		code := RenderFontFileTemplate(f, gs)
		fmt.Println("Writing to", f.RustOut)
		ioutil.WriteFile(f.RustOut, []byte(code), 0644)
		loader := RenderLoaderFileTemplate(f, gs)
		fmt.Println("Writing to", f.LoaderOut)
		ioutil.WriteFile(f.LoaderOut, []byte(loader), 0644)
		summary := lib.FontSummary{
			Name: strings.ToLower(f.Name),
			Len:  gs.GlyphsLen,
		}
		fdir = append(fdir, summary)

		offset := lib.FontMap{
			Name: strings.ToUpper(f.Name) + "_OFFSET",
			Len:  fmt.Sprintf("%x", cur_address),
		}
		offsets = append(offsets, offset)
		if f.Small {
			small_offsets = append(small_offsets, offset)
		}
		length := lib.FontMap{
			Name: strings.ToUpper(f.Name) + "_LEN",
			Len:  fmt.Sprintf("%x", gs.GlyphsLen*4),
		}
		offsets = append(offsets, length)
		if f.Small {
			small_offsets = append(small_offsets, length)
		}
		cur_address = cur_address + gs.GlyphsLen*4
		if f.Small {
			small_cur_address = small_cur_address + gs.GlyphsLen*4
		}
	}
	total := lib.FontMap{
		Name: "FONT_TOTAL_LEN",
		Len:  fmt.Sprintf("%x", cur_address),
	}
	offsets = append(offsets, total)
	small_total := lib.FontMap{
		Name: "FONT_TOTAL_LEN",
		Len:  fmt.Sprintf("%x", small_cur_address),
	}
	small_offsets = append(small_offsets, small_total)
	for _, fs := range fdir {
		fmt.Println(fs)
	}
	loadermod := RenderLoadermodTemplate(fdir)
	fmt.Println("Writing to", conf.GetLoaderMod())
	ioutil.WriteFile(conf.GetLoaderMod(), []byte(loadermod), 0644)

	fontmap := RenderFontmapTemplate(offsets, small_offsets)
	fmt.Println("Writing to", conf.GetFontMap())
	ioutil.WriteFile(conf.GetFontMap(), []byte(fontmap), 0644)
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
	fmt.Println("Metafile: ", conf.GetLoaderMod())
	fmt.Println("Metafile: ", conf.GetFontMap())
}
