# Sound Garden book. Run from this directory, or `make -C book <target>` from the repo root.
#
#   make          draft HTML (with the typesetting sampler) in build/, then open it
#   make html     draft HTML only
#   make final    HTML without draft material
#   make check    strict build: any warning (unknown word, clipping, broken xref) fails
#   make watch    rebuild on every save
#   make clean    remove build/ (renders are recreated on the next build)

BUILD    := $(CURDIR)/build
RENDERER := ../target/release/book_render
ASCIIDOC := asciidoctor -r ./ext/sound_garden.rb -D $(BUILD) -o index.html
SOURCES  := $(shell find . -name '*.adoc' -not -path './build/*') $(wildcard ext/* theme/* examples/*/*)

.PHONY: all html final check watch open clean renderer

all: html open

html: renderer
	$(ASCIIDOC) -a draft book.adoc

final: renderer
	$(ASCIIDOC) book.adoc

check: renderer
	$(ASCIIDOC) -a draft --failure-level WARN book.adoc

open:
	open $(BUILD)/index.html

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

clean:
	rm -rf $(BUILD)
