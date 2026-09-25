#![cfg(windows)]
//! Regression: an argument holding `"`, `'` or `{}` has to reach an MSYS2 child
//! (Git Bash's grep, ls, wc) intact. std quoted it for the MSVC runtime only, and the
//! MSYS2 runtime rebuilt a different argv: `rtk grep -c -E '"(alias|dossier)"'
//! file` printed 0 where grep prints 2. Skipped when no Git for Windows grep is
//! around.

use std::path::{Path, PathBuf};
use std::process::Command;

/// `git.exe` sits in `<root>/cmd`, `<root>/mingw64/bin` or `<root>/bin`, so walk
/// up until an ancestor also has `usr/bin/grep.exe` under it.
fn msys_grep() -> Option<PathBuf> {
    let git = which::which("git").ok()?;
    git.ancestors()
        .map(|root| root.join("usr").join("bin").join("grep.exe"))
        .find(|candidate| candidate.is_file())
}

const FIXTURE: &str = r#"{
  "alias": ["documentation juridique"],
  "dossier": "C:\\Projets\\codeur",
  "note": "l'offre du dossier, 2026"
}
"#;

/// Run the built rtk in a directory holding the fixture, with only MSYS2's
/// `usr/bin` on PATH: rg is then missing and `rtk grep` falls back to grep.
/// `msys` is the `MSYS` option string rtk and grep inherit (None: unset). A
/// user's own TOML filters are bypassed, so the output is grep's alone.
fn rtk(
    grep_dir: &Path,
    work: &Path,
    msys: Option<&str>,
    args: &[&str],
) -> (Option<i32>, String, String) {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_rtk"));
    cmd.args(args)
        .current_dir(work)
        .env("PATH", grep_dir)
        .env("RTK_DB_PATH", work.join("tracking.db"))
        .env("RTK_TELEMETRY_DISABLED", "1")
        .env("RTK_NO_TOML", "1");
    match msys {
        Some(options) => cmd.env("MSYS", options),
        None => cmd.env_remove("MSYS"),
    };
    let out = cmd.output().expect("failed to run rtk");
    (
        out.status.code(),
        String::from_utf8_lossy(&out.stdout).trim().to_string(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn quoted_patterns_reach_msys_grep_intact() {
    let Some(grep) = msys_grep() else {
        eprintln!("skipping: no Git for Windows grep found");
        return;
    };
    let grep_dir = grep.parent().expect("grep.exe has a parent directory");
    if !grep_dir.join("msys-2.0.dll").is_file() {
        eprintln!("skipping: {} is not an MSYS2 program", grep.display());
        return;
    }

    let work = tempfile::tempdir().expect("temp dir");
    std::fs::write(work.path().join("client.json"), FIXTURE).expect("write fixture");
    std::fs::write(work.path().join("l'offre.txt"), "une ligne\nune autre\n")
        .expect("write fixture");

    let cases: [(Option<&str>, &[&str], &str); 13] = [
        // The reported command: clap rejects the leading -c, rtk runs grep as is.
        (
            None,
            &["grep", "-c", "-E", r#""(alias|dossier)""#, "client.json"],
            "2",
        ),
        // Pattern first: rtk grep's own path, grep fallback since rg is missing.
        (None, &["grep", r#""alias""#, "client.json", "-c"], "1"),
        // `'` opened a quoted run that swallowed the file name.
        (None, &["grep", "-c", "l'offre", "client.json"], "1"),
        // Brace expansion turned `[0-9]{4}` into `[0-9]4`.
        (None, &["grep", "-c", "-E", "[0-9]{4}", "client.json"], "1"),
        // The raw escape hatch goes through the same encoding.
        (
            None,
            &[
                "proxy",
                "grep",
                "-c",
                "-E",
                r#""(alias|dossier)""#,
                "client.json",
            ],
            "2",
        ),
        // A quote next to escaped backslashes, as in a JSON path: each `\` is
        // escaped on the command line and read back once.
        (
            None,
            &["grep", "-c", "-F", r#""C:\\Projets"#, "client.json"],
            "1",
        ),
        // Under noglob the runtime reads no escape at all, hence its own encoding.
        (
            Some("noglob"),
            &["grep", "-c", "-E", r#""(alias|dossier)""#, "client.json"],
            "2",
        ),
        (
            Some("noglob"),
            &["grep", "-c", "-F", r#""C:\\Projets"#, "client.json"],
            "1",
        ),
        // A wildcard reaches the program as rtk got it: expanding it was the
        // caller's shell's job, not something MSYS2 does behind its back.
        (None, &["proxy", "printf", "%s", "*.json"], "*.json"),
        // rtk ls and rtk wc hand the file name over the same way.
        (None, &["ls", "l'offre.txt"], "l'offre.txt  20B"),
        (None, &["wc", "-l", "l'offre.txt"], "2"),
        // An argument the runtime reads intact keeps std's bytes, in both modes.
        (
            None,
            &["grep", "-c", "-F", r"C:\\Projets", "client.json"],
            "1",
        ),
        (
            Some("noglob"),
            &["grep", "-c", "-F", r"C:\\Projets", "client.json"],
            "1",
        ),
    ];
    for (msys, args, want) in cases {
        let (code, stdout, stderr) = rtk(grep_dir, work.path(), msys, args);
        assert_eq!(
            (code, stdout.as_str()),
            (Some(0), want),
            "MSYS={msys:?} rtk {args:?}\nstderr: {stderr}"
        );
    }
}
