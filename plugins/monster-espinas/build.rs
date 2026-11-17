use std::{env, fmt::Write as _, fs, path::PathBuf};

fn main() {
    let dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    println!("cargo:rustc-link-arg=-T{dir}/link/switch32.ld");
    println!("cargo:rustc-link-arg=--build-id=sha1");
    println!("cargo:rerun-if-changed=link/switch32.ld");
    println!("cargo:rerun-if-changed=materials.tsv");
    let mut rows = String::from("const MATERIALS: &[Material] = &[\n");
    for line in include_str!("materials.tsv")
        .lines()
        .filter(|l| !l.starts_with('#'))
    {
        let fields: Vec<_> = line.split('\t').collect();
        assert_eq!(fields.len(), 15);
        let id: u16 = fields[0].parse().unwrap();
        let fz: u16 = fields[1].parse().unwrap();
        assert_eq!(fz - 1046, id - 2980);
        let [icon, color, rarity, stack] =
            core::array::from_fn(|i| fields[2 + i].parse::<u8>().unwrap());
        let sell: u32 = fields[6].parse().unwrap();
        let buy: u32 = fields[7].parse().unwrap();
        write!(rows, "Material {{ id: {id}, icon:{icon}, color:{color}, rarity:{rarity}, stack:{stack}, sell:{sell}, buy:{buy}, names: [").unwrap();
        for name in &fields[8..] {
            assert!(!name.contains('\0'));
            write!(rows, "c{name:?},").unwrap();
        }
        rows.push_str("] },\n");
    }
    rows.push_str("];\n");
    fs::write(
        PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("materials.rs"),
        rows,
    )
    .unwrap();
}
