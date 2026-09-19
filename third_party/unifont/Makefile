# glyphs.txt holds, for every character in chars_to_include.txt, 16 lines
# of "glyph_XXXX.txt:" plus 16 pixels, sorted by code point. It is cut
# out of the Unifont Japanese BDF by the imedict tool in the book
# repository (manuscript_os/imedict), which reads the font once:
#
#   cd ../../../imedict && cargo run --release -- --repo ../code \
#       --bdf ../code/third_party/unifont/unifont_jp-16.0.04.bdf [inputs...]
#
# imedict downloads the BDF to that path if it is missing; this rule
# does the same for use by hand.
unifont_jp-16.0.04.bdf :
	wget https://unifoundry.com/pub/unifont/unifont-16.0.04/font-builds/unifont_jp-16.0.04.bdf.gz
	gunzip unifont_jp-16.0.04.bdf.gz
