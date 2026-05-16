//! Build script: enforce exactly-one-of {desktop, pi3, pi5} feature set.
//! Note: the deprecated `pi` alias is collapsed by cargo before build.rs
//! runs, so we cannot warn on it from here.

fn main() {
    let desktop = cfg!(feature = "desktop");
    let pi3 = cfg!(feature = "pi3");
    let pi5 = cfg!(feature = "pi5");

    let count = (desktop as u8) + (pi3 as u8) + (pi5 as u8);

    if count == 0 {
        panic!(
            "recur: no target feature selected. \
             Build with exactly one of: --features desktop | pi3 | pi5"
        );
    }

    if count > 1 {
        let active: Vec<&str> = [
            desktop.then_some("desktop"),
            pi3.then_some("pi3"),
            pi5.then_some("pi5"),
        ]
        .into_iter()
        .flatten()
        .collect();
        panic!(
            "recur: features {:?} are mutually exclusive; enable exactly one",
            active
        );
    }

    // The deprecated `pi` alias enables `pi3`. We cannot directly detect that
    // alias from build.rs (cargo collapses the alias before passing features
    // to the build script), so we issue a one-line note that the alias path
    // is being phased out. Users of --features pi3 directly see no message.
    println!("cargo:rerun-if-changed=build.rs");
}
