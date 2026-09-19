"""Build publishable archives and compile a consumer against their extracted contents."""
from pathlib import Path
import os
import shutil
import subprocess
import tarfile
import tempfile
import tomllib

root = Path(__file__).resolve().parents[1]
version = tomllib.loads((root / "Cargo.toml").read_text(encoding="utf-8"))["workspace"]["package"]["version"]
packages = [tomllib.loads(manifest.read_text())["package"] for manifest in sorted((root / "crates").glob("*/Cargo.toml"))]
names = [package["name"] for package in packages if package.get("publish") is not False]
selection = [flag for name in names for flag in ("-p", name)]
subprocess.run(["cargo", "package", *selection, "--locked", "--allow-dirty", "--no-verify"], cwd=root, check=True)
# A fresh consumer avoids Cargo's temporary-registry cache retaining an older
# archive when contributors package the same unpublished version repeatedly.
with tempfile.TemporaryDirectory(prefix="wove-package-") as directory:
    consumer = Path(directory)
    for name in names:
        with tarfile.open(root / f"target/package/{name}-{version}.crate") as archive:
            archive.extractall(consumer, filter="data")
    (consumer / "Cargo.toml").write_text(f'''[package]
name = "consumer"
version = "0.0.0"
edition = "2021"
[features]
default = ["terminal", "markdown", "syntax", "diff"]
terminal = ["wove/terminal", "wove-dioxus/terminal", "dep:wove-ssh"]
markdown = ["wove/markdown"]
syntax = ["wove/syntax"]
diff = ["wove/diff"]
[dependencies]
wove = {{ path = "wove-{version}", default-features = false }}
wove-keymap = {{ path = "wove-keymap-{version}" }}
wove-dioxus = {{ path = "wove-dioxus-{version}", default-features = false }}
wove-ssh = {{ path = "wove-ssh-{version}", optional = true }}
[patch.crates-io]
wove = {{ path = "wove-{version}" }}
''', encoding="utf-8")
    (consumer / "src").mkdir()
    (consumer / "src/main.rs").write_text('''use wove::{Tree, elements::Text};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut tree = Tree::new();
    tree.add(tree.root(), Text::new("packed consumer"))?;
    let frame = tree.frame(20, 2)?;
    assert_eq!(frame.cell(0, 0).unwrap().symbol(), "p");
    #[cfg(feature = "markdown")]
    let _markdown = wove::markdown::render("# Packed", wove::markdown::Palette::default());
    #[cfg(feature = "syntax")]
    let _syntax = wove::syntax::Syntaxes::new();
    #[cfg(feature = "diff")]
    let _diff = wove::diff::render("old", "new", wove::Style::default(), wove::Style::default(), wove::Style::default());
    let _keys = wove_keymap::Keymap::<()>::new(std::time::Duration::from_millis(300));
    let _registry = wove_dioxus::Registry::default();
    #[cfg(feature = "terminal")]
    let _remote = std::mem::size_of::<wove_ssh::Server>();
    // Rendering to bytes needs no terminal backend, so it runs in both builds.
    wove::Renderer::default().draw(&mut Vec::new(), frame)?;
    Ok(())
}
''', encoding="utf-8")
    shutil.copy2(root / "Cargo.lock", consumer / "Cargo.lock")
    environment = {**os.environ, "CARGO_TARGET_DIR": str(root / "target/consumer")}
    for features in ([], ["--no-default-features"]):
        subprocess.run(["cargo", "run", "--offline", *features], cwd=consumer, env=environment, check=True)
