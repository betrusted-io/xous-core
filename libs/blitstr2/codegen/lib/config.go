// Copyright (c) 2022 Sam Blenny
// SPDX-License-Identifier: Apache-2.0 OR MIT
//
package lib

import (
	"encoding/json"
	"fmt"
	"io/ioutil"
	"strings"
)

// Holds data from top level of JSON config file
type Config struct {
	Comment   []string
	LoaderMod []string
	FontMap   []string
	GlyphSets []ConfigGlyphSet
}

// Holds data from elements of {GlyphSets:[...]} from the JSON config file
type ConfigGlyphSet struct {
	Name      string
	Sprites   string
	Size      int
	Cols      int
	Gutter    int
	Border    int
	Legal     string
	Index     string
	IndexType string
	GlyphTrim string
	RustOut   string
	LoaderOut string
	Small     bool
}

// Holds data parsed from a json index file
type ConfigJsonIndex struct {
	Comment []string
	Map     []CharSpec
}

// Read the config file to make a config object
func NewConfig(configFile string) Config {
	data, err := ioutil.ReadFile(configFile)
	if err != nil {
		panic(err)
	}
	var config Config
	err = json.Unmarshal(data, &config)
	if err != nil {
		panic(err)
	}
	return config
}

func (c Config) GetLoaderMod() string {
	return c.LoaderMod[0]
}
func (c Config) GetFontMap() string {
	return c.FontMap[0]
}

// Generate font glyph set specifications with character maps, aliases, etc.
func (c Config) Fonts() []FontSpec {
	list := []FontSpec{}
	for _, gs := range c.GlyphSets {
		fs := FontSpec{
			gs.Name, gs.Sprites, gs.Size, gs.Cols, gs.Gutter, gs.Border,
			gs.readLegal(),
			gs.codepointMap(),
			gs.RustOut, gs.GlyphTrim,
			gs.LoaderOut,
			gs.Small,
		}
		list = append(list, fs)
	}
	return list
}

// Read the legal notice for a config glyph set
func (c ConfigGlyphSet) readLegal() string {
	if c.Legal != "" {
		data, err := ioutil.ReadFile(c.Legal)
		if err != nil {
			panic(err)
		}
		return strings.TrimSpace(string(data))
	} else {
		return ""
	}
}

// Generate a list of codepoint to sprite grid coordinate mappings
func (c ConfigGlyphSet) codepointMap() []CharSpec {
	switch c.IndexType {
	case "txt-row-major":
		return CJKMap(c.Cols, c.Index)
	case "json-grid-coord":
		return c.readJsonCodepointList()
	default:
		panic(fmt.Errorf("bad indexType: %s", c.IndexType))
	}
}

// Generate a codepoint list from a config glyph set json index file
func (c ConfigGlyphSet) readJsonCodepointList() []CharSpec {
	data, err := ioutil.ReadFile(c.Index)
	if err != nil {
		panic(err)
	}
	var cji ConfigJsonIndex
	err = json.Unmarshal(data, &cji)
	if err != nil {
		panic(err)
	}
	return cji.Map
}
