//! `riffcat-core` with a producer that is not Rust: the files in
//! `examples/outside-producer` are written by a Python script from the JSON
//! format alone. No recipe is compiled into the tool.

use std::path::{Path, PathBuf};
use std::process::Command;

fn example() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/outside-producer")
}

struct Run {
    corpus: PathBuf,
}

impl Run {
    fn new(tag: &str) -> Self {
        let corpus = std::env::temp_dir().join(format!("riffcat-outside-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&corpus);
        Self { corpus }
    }

    fn core(&self, args: &[&str]) -> (bool, String) {
        let output = Command::new(env!("CARGO_BIN_EXE_riffcat-core"))
            .arg("--corpus")
            .arg(&self.corpus)
            .args(args)
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&output.stdout).into_owned() + &String::from_utf8_lossy(&output.stderr);
        (output.status.success(), text)
    }

    fn ok(&self, args: &[&str]) -> String {
        let (success, text) = self.core(args);
        assert!(success, "riffcat-core {args:?} failed:\n{text}");
        text
    }
}

impl Drop for Run {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.corpus);
    }
}

fn file(name: &str) -> String {
    example().join(name).display().to_string()
}

/// The groups of at least two units a `bucket --json` run reports: the
/// group's size, how many batches it spans, and its members' names.
fn groups(run: &Run, facet: &str) -> Vec<(String, String, String)> {
    let text = run.ok(&["--json", "bucket", "--unit", "calc-expr", "--facet", facet]);
    let rows: Vec<serde_json::Value> = serde_json::from_str(&text).unwrap();
    let cell = |row: &serde_json::Value, name: &str| row[name].as_str().unwrap().to_string();
    let mut groups: Vec<_> =
        rows.iter().map(|row| (cell(row, "members"), cell(row, "size"), cell(row, "artifacts"))).collect();
    groups.sort();
    groups
}

#[test]
fn a_python_producer_is_admitted_grouped_and_committed_to() {
    let run = Run::new("flow");
    let (declaration, left, right) = (file("calc-1.json"), file("left.jsonl"), file("right.jsonl"));

    assert!(run.ok(&["check", "--declaration", &declaration, &left, &right]).contains("6 units checked, 0 break"));
    // Checking admits nothing and stores nothing.
    assert!(!run.ok(&["recipes"]).contains("calc/1"));

    run.ok(&["add", "--declaration", &declaration, "--world", "left", &left]);
    // The corpus now holds the declaration: the second world needs no file.
    run.ok(&["add", "--world", "right", &right]);
    assert_eq!(run.ok(&["recipes"]).trim(), "calc/1");

    // Each group is one expression's two units, one from each world.
    let pair = |name: &str| (name.to_string(), "2".to_string(), "2".to_string());
    assert_eq!(groups(&run, "all"), [pair("area")]);
    assert_eq!(groups(&run, "names-blind"), [pair("area"), pair("scale")]);
    assert_eq!(groups(&run, "structure"), [pair("area"), pair("offset"), pair("scale")]);

    // The root is a function of what was admitted, not of how often.
    let root = run.ok(&["root"]);
    run.ok(&["add", "--world", "right", &right]);
    assert_eq!(run.ok(&["root"]), root);
    let hex = root.split_whitespace().find(|word| word.len() == 64).expect("a root digest").to_string();
    run.ok(&["root", "--check", &hex]);
}

#[test]
fn units_are_refused_without_or_against_their_declaration() {
    let run = Run::new("refuse");
    let (declaration, left) = (file("calc-1.json"), file("left.jsonl"));

    let (success, text) = run.core(&["add", "--world", "left", &left]);
    assert!(!success && text.contains("no declaration for recipe calc/1"), "{text}");

    // A unit with a field its declaration does not have.
    let broken = std::env::temp_dir().join(format!("riffcat-outside-broken-{}.jsonl", std::process::id()));
    let text = std::fs::read_to_string(example().join("left.jsonl")).unwrap();
    std::fs::write(&broken, text.replace("\"name\":\"op\"", "\"name\":\"operator\"")).unwrap();
    let (success, text) = run.core(&["add", "--declaration", &declaration, "--world", "left", broken.to_str().unwrap()]);
    assert!(!success && text.contains("breaks the calc/1 declaration"), "{text}");
    let (success, _) = run.core(&["check", "--declaration", &declaration, broken.to_str().unwrap()]);
    assert!(!success);
    // Nothing was admitted, so nothing was stored either.
    assert!(run.ok(&["recipes"]).trim().is_empty());
    std::fs::remove_file(broken).unwrap();
}

/// The checked-in files are what the script writes today.
#[test]
fn the_checked_in_files_are_the_scripts_output() {
    let out = std::env::temp_dir().join(format!("riffcat-outside-regen-{}", std::process::id()));
    let Ok(status) = Command::new("python3").arg(example().join("producer.py")).arg(&out).status() else {
        eprintln!("python3 not found; skipping");
        return;
    };
    assert!(status.success());
    for name in ["calc-1.json", "left.jsonl", "right.jsonl"] {
        assert_eq!(
            std::fs::read_to_string(out.join(name)).unwrap(),
            std::fs::read_to_string(example().join(name)).unwrap(),
            "{name} is stale: run producer.py"
        );
    }
    std::fs::remove_dir_all(out).unwrap();
}
