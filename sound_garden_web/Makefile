# Browser build: the engine (wasm), the worklet, the page API and the playground.
#
#   make -C sound_garden_web        build dist/
#   make -C sound_garden_web serve  serve dist/ on http://localhost:8124

DIST := $(CURDIR)/dist
WASM := ../target/wasm32-unknown-unknown/release/sound_garden_web.wasm

.PHONY: all serve clean

all:
	cargo build --quiet --release --target wasm32-unknown-unknown -p sound_garden_web
	mkdir -p $(DIST)
	cp $(WASM) web/*.js web/*.html $(DIST)/
	@printf "engine: %s KB, %s KB gzipped\n" $$(( $$(wc -c < $(DIST)/sound_garden_web.wasm) / 1024 )) $$(( $$(gzip -9c $(DIST)/sound_garden_web.wasm | wc -c) / 1024 ))

serve: all
	python3 -m http.server 8124 -d $(DIST)

clean:
	rm -rf $(DIST)
