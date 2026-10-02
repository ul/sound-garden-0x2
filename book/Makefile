# Sound Garden book. Run from this directory, or `make -C book <target>` from the repo root.
#
#   make          draft HTML (with the typesetting sampler) in build/, then open it
#   make html     draft HTML only
#   make final    HTML without draft material
#   make check    strict build: any warning (unknown word, clipping, broken xref) fails
#   make watch    rebuild on every save
#   make serve    build and serve on http://localhost:8123 (needed for live editing)
#   make notes    render private working notes and lessons (notes/*.adoc) into build/notes/
#   make anki     build the private Anki deck (notes/anki/*.toml) into build/notes/sound-garden.apkg
#   make clean    remove build/ (renders are recreated on the next build)

BUILD    := $(CURDIR)/build
RENDERER := ../target/release/book_render
ASCIIDOC := asciidoctor -r ./ext/sound_garden.rb -D $(BUILD) -o index.html
SOURCES  := $(shell find . -name '*.adoc' -not -path './build/*') $(wildcard ext/* theme/* examples/*/*)

.PHONY: all html final check watch open serve clean renderer live notes anki

all: html open

html: renderer live
	$(ASCIIDOC) -a draft book.adoc

final: renderer live
	$(ASCIIDOC) book.adoc

check: renderer
	$(ASCIIDOC) -a draft --failure-level WARN book.adoc

open:
	open $(BUILD)/index.html

# Notes are gitignored (private); they render with the book's theme and extension, and share its
# stylesheet and live engine from the main build, so build the book first.
notes: html
	@mkdir -p $(BUILD)/notes && cp -R $(BUILD)/live $(BUILD)/notes/
	asciidoctor -r ./ext/sound_garden.rb -D $(BUILD)/notes \
	  -a stylesdir=../theme -a copycss! -a docinfodir=../theme notes/*.adoc

anki: renderer
	uv run --quiet --with genanki python3 notes/anki/build.py

# Live editing needs http: browsers don't load audio worklets from file://.
serve: html
	python3 -m http.server 8123 -d $(BUILD)

watch: html
	@echo "Watching for changes; Ctrl-C to stop."
	@while true; do \
	  sleep 1; \
	  if [ -n "$$(find . ../audio_program/src/help.adoc -newer $(BUILD)/index.html \( -name '*.adoc' -o -name '*.txt' -o -name '*.css' -o -name '*.html' -o -name '*.rb' \) -not -path './build/*' | head -1)" ]; then \
	    $(ASCIIDOC) -a draft book.adoc && echo "rebuilt $$(date +%T)"; \
	  fi; \
	done

renderer:
	@cargo build --quiet --release -p book_render

# The browser engine for live editing: just the sound_garden_web engine, not the editor.
live:
	@cargo build --locked --quiet --release --target wasm32-unknown-unknown -p sound_garden_web
	@mkdir -p $(BUILD)/live
	@cp ../target/wasm32-unknown-unknown/release/sound_garden_web.wasm \
	    ../sound_garden_web/web/sound-garden.js ../sound_garden_web/web/worklet.js $(BUILD)/live/

clean:
	rm -rf $(BUILD)
