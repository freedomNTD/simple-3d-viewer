fn main() {
    #[cfg(windows)]
    {
        // the viewer's Windows FFI (drop handling, registry, dialogs) pulls in
        // ole32 (OleRevokeDropTarget) and shell32 (SHChangeNotify) beyond what
        // the dependency graph already links
        println!("cargo:rustc-link-lib=ole32");
        println!("cargo:rustc-link-lib=shell32");
    }
}
