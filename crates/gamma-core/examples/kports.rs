//! Print the ports gdsfactory stored in a GDS file: `cargo run -p gamma-core --example kports -- file.gds`.
fn main() {
    let p = std::env::args().nth(1).expect("a GDS file");
    let lib = gamma_core::gds::Library::read(std::path::Path::new(&p)).unwrap();
    let tops = lib.top_cells();
    println!("top cells {tops:?}");
    for (k, v) in &lib.meta {
        println!("metadata of '{k}': {} entries", v.len());
    }
    println!("{:?}", lib.kfactory_ports(tops[0]));
}
