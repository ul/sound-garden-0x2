# Sound Garden book. Run from this directory, or `make -C book <target>` from the repo root.
#
#   make          draft HTML (with the typesetting sampler) in build/, then open it
#   make html     draft HTML only
#   make final    HTML without draft material
#   make check    strict build: any warning (unknown word, clipping, broken xref) fails
#   make watch    rebuild on every save
#   make serve    build and serve on http://localhost:8123 (needed for live editing)
#   make clean    remove build/ (renders are recreated on the next build)

BUILD    := $(CURDIR)/build
RENDERER := ../target/release/book_render
ASCIIDOC := asciidoctor -r ./ext/sound_garden.rb -D $(BUILD) -o index.html
SOURCES  := $(shell find . -name '*.adoc' -not -path './build/*') $(wildcard ext/* theme/* examples/*/*)

.PHONY: all html final check watch open serve clean renderer live

all: html open

html: renderer live
	$(ASCIIDOC) -a draft book.adoc

final: renderer live
	$(ASCIIDOC) book.adoc

check: renderer
	$(ASCIIDOC) -a draft --failure-level WARN book.adoc

open:
	open $(BUILD)/index.html

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

# The browser engine for live editing (sound_garden_web).
live:
	@$(MAKE) --quiet -C ../sound_garden_web >/dev/null
	@mkdir -p $(BUILD)/live
	@cp ../sound_garden_web/dist/sound_garden_web.wasm ../sound_garden_web/dist/*.js $(BUILD)/live/

clean:
	rm -rf $(BUILD)
