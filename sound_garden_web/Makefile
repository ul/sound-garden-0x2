# Browser build: AudioWorklet engine, text playground and shared egui editor.
#
#   make -C sound_garden_web        build dist/
#   make -C sound_garden_web serve  serve dist/ on http://localhost:8124

DIST := $(CURDIR)/dist
ENGINE_WASM := ../target/wasm32-unknown-unknown/release/sound_garden_web.wasm
EDITOR_WASM := ../target/wasm32-unknown-unknown/release/sound_garden_egui.wasm
WASM_BINDGEN ?= wasm-bindgen

.PHONY: all serve clean

all:
	cargo build --locked --quiet --release --target wasm32-unknown-unknown -p sound_garden_web
	cargo build --locked --quiet --release --target wasm32-unknown-unknown -p sound_garden_egui --lib
	mkdir -p $(DIST)/editor/pkg
	cp $(ENGINE_WASM) web/*.js web/*.html ../README.html $(DIST)/
	cp -R web/editor/. $(DIST)/editor/
	$(WASM_BINDGEN) --target web --out-dir $(DIST)/editor/pkg $(EDITOR_WASM)
	@printf "engine: %s KB, editor: %s KB\n" $$(( $$(wc -c < $(DIST)/sound_garden_web.wasm) / 1024 )) $$(( $$(wc -c < $(DIST)/editor/pkg/sound_garden_egui_bg.wasm) / 1024 ))

serve: all
	python3 -m http.server 8124 -d $(DIST)

clean:
	rm -rf $(DIST)
