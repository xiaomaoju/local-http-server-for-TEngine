use std::fs;
use std::path::Path;

fn main() {
    // Ensure web-admin/dist exists so rust-embed can compile even without a frontend build
    let dist = Path::new("../web-admin/dist");
    if !dist.exists() {
        fs::create_dir_all(dist).expect("Failed to create web-admin/dist");
    }
}
