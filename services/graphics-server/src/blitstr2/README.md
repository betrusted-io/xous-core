# blitstr2

This code is vendored in from https://github.com/samblenny/blitstr2 with permission from the author.

multi-lingual string blitter for 1-bit monochrome (sequel to blitstr)


## What's New

Compared to the original version of blitstr, blitstr2:
- Supports more languages
- Uses 16x16 pixel size for CJK glyphs instead of 32x32
- Makes it easier for calling code to calculate size of rendered strings
- Ignores multi-codepoint grapheme clusters for modern emoji (instead, there
  is limited support for old-school monochrome bitmap emoji)
- Focuses on an API to look up glyphs from just one typeface at a time


## License

Source code for blitstr2 is dual licensed under the terms of [Apache 2.0](LICENSE-APACHE)
or [MIT](LICENSE-MIT), at your option.

Glyph bitmaps included with blitstr2 have their own copyrights and licenses
(OFL-1.1, public domain, Japanese equivalent of public domain).

See [LEGAL.md](LEGAL.md) for copyright and license details on embedded glyph
bitmaps.
