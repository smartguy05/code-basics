fn main() {
    // libgit2's vendored C code calls registry (RegOpenKeyExW) and CryptoAPI
    // (CryptGenRandom) functions, but libgit2-sys emits no link directive for
    // advapi32 and current Rust std no longer links it implicitly. Without
    // this, every binary linking cb-core fails with LNK2019 on Windows.
    if std::env::var_os("CARGO_CFG_WINDOWS").is_some() {
        println!("cargo:rustc-link-lib=advapi32");
    }
}
