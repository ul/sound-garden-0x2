use sound_garden_format::NodeRepository;

fn main() {
    let mut args = std::env::args().skip(1);
    let path = &args.next().expect("Please provide input path.");
    if !std::path::Path::new(path).is_file() {
        eprintln!("{} is not a file.", path);
    }
    println!(
        "{}",
        NodeRepository::load(&path)
            .nodes()
            .into_iter()
            .map(|node| node.text)
            .collect::<Vec<_>>()
            .join(" ")
    );
}
