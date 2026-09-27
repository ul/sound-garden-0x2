#[cfg(not(target_arch = "wasm32"))]
fn main() -> anyhow::Result<()> {
    sound_garden_egui::native_main()
}

// The browser loads the library through wasm-bindgen, not this binary.
#[cfg(target_arch = "wasm32")]
fn main() {}
