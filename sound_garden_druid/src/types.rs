pub type Id = u64;

pub fn id_to_string(id: Id) -> String {
    format!("{:016x}", id)
}

pub fn str_to_id(id: &str) -> Id {
    Id::from_str_radix(id, 16).unwrap()
}
