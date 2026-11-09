//! End-to-end tests: run the real `ripplefix` binary against a temporary Git
//! repository built from `examples/fastapi-pydantic-ts`.

use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/fastapi-pydantic-ts")
}

fn copy_dir(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).unwrap();
    for entry in std::fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let target = dst.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

fn git(dir: &Path, args: &[&str]) {
    let ok = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@example.com",
            "-c",
            "core.autocrlf=false",
        ])
        .args(args)
        .status()
        .unwrap()
        .success();
    assert!(ok, "git {args:?} failed");
}

fn repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    copy_dir(&fixture(), dir.path());
    git(dir.path(), &["init", "-q"]);
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-qm", "init"]);
    dir
}

fn edit(dir: &Path, rel: &str, from: &str, to: &str) {
    let p = dir.join(rel);
    let s = std::fs::read_to_string(&p).unwrap();
    assert!(s.contains(from), "fixture missing {from:?}");
    std::fs::write(p, s.replacen(from, to, 1)).unwrap();
}

fn ripplefix(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ripplefix"))
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .unwrap()
}

fn analyze_json(dir: &Path) -> Value {
    let out = ripplefix(dir, &["analyze", "--format", "json"]);
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap()
}

#[test]
fn clean_worktree_reports_nothing() {
    let r = repo();
    let v = analyze_json(r.path());
    assert_eq!(v["files"].as_array().unwrap().len(), 0);
}

#[test]
fn pydantic_field_rename_is_detected_as_inferred_and_breaking() {
    let r = repo();
    edit(
        r.path(),
        "backend/app/models.py",
        "full_name: str",
        "display_name: str",
    );
    let v = analyze_json(r.path());
    let files = v["files"].as_array().unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0]["status"], "structural");
    let changes = files[0]["changes"].as_array().unwrap();
    assert_eq!(changes.len(), 1, "{changes:#?}");
    let c = &changes[0];
    assert_eq!(c["model"], "UserBase");
    assert_eq!(c["type"], "field_renamed");
    assert_eq!(c["old"], "full_name");
    assert_eq!(c["new"], "display_name");
    assert_eq!(c["confidence"], "inferred");
    assert_eq!(c["breaks_wire_contract"], true);
    assert_eq!(c["evidence"][0]["file"], "backend/app/models.py");
    assert_eq!(c["evidence"][0]["span"]["start_line"], 8);
}

#[test]
fn formatting_and_comment_edits_are_cosmetic() {
    let r = repo();
    edit(
        r.path(),
        "backend/app/models.py",
        "    email: str\n",
        "    # primary contact\n    email:   str\n\n",
    );
    let v = analyze_json(r.path());
    assert_eq!(v["files"][0]["status"], "cosmetic");
    assert_eq!(v["files"][0]["changes"].as_array().unwrap().len(), 0);
}

#[test]
fn syntax_error_is_reported_not_guessed() {
    let r = repo();
    edit(
        r.path(),
        "backend/app/models.py",
        "class UserRead(UserBase):",
        "class UserRead(UserBase:",
    );
    let v = analyze_json(r.path());
    assert_eq!(v["files"][0]["status"], "unanalyzable");
}

#[test]
fn fail_on_breaking_sets_exit_code_and_non_python_files_are_counted() {
    let r = repo();
    edit(r.path(), "backend/app/models.py", "id: int", "id: str");
    edit(r.path(), "frontend/src/api.ts", "id: number", "id: string");
    let out = ripplefix(r.path(), &["analyze", "--fail-on-breaking"]);
    assert_eq!(out.status.code(), Some(1));
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        text.contains("UserRead: field type changed: id: int -> str (confirmed) [breaking]"),
        "{text}"
    );
    assert!(text.contains("1 changed file(s) not supported"), "{text}");
}

#[test]
fn invalid_base_revision_fails_cleanly() {
    let r = repo();
    let out = ripplefix(r.path(), &["analyze", "--base=--upload-pack=evil"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("invalid revision"));
}

#[test]
fn untracked_python_files_are_analyzed() {
    let r = repo();
    std::fs::write(
        r.path().join("backend/app/extra.py"),
        "from pydantic import BaseModel\nclass Team(BaseModel):\n    name: str\n",
    )
    .unwrap();
    let v = analyze_json(r.path());
    assert_eq!(v["files"][0]["changes"][0]["type"], "model_added");
    assert_eq!(v["files"][0]["delta"]["status"], "untracked");
}
