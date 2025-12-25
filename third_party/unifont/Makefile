GLYPHS_TO_INCLUDE=$(shell cat chars_to_include.txt | \
				 iconv -f utf-8 -t utf16be | xxd -p | grep -o '....' | \
				 sort | tr '[:lower:]' '[:upper:]' | grep -v 000A | \
				 sed -E -e 's/^/glyph_/' -e 's/$$/.txt/')

glyphs.txt: $(GLYPHS_TO_INCLUDE) Makefile
	grep '$$' glyph_*.txt | tee glyphs.txt

unifont_jp-16.0.04.bdf :
	wget https://unifoundry.com/pub/unifont/unifont-16.0.04/font-builds/unifont_jp-16.0.04.bdf.gz
	gunzip unifont_jp-16.0.04.bdf.gz

glyph_%.txt : unifont_jp-16.0.04.bdf Makefile
	grep -A30 "STARTCHAR.*$*" unifont_jp-16.0.04.bdf | grep -B16 ENDCHAR | head -n 16 | xargs -I {} -- bash -c 'echo "obase=2; ibase=16; {}" | bc | sed 's/^/0000000000000000/'' | grep -o -E ".{16}$$" | tr '1' '#' | tr '0' '.' > $@
