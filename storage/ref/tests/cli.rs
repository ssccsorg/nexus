// The contract a developer or an auditor runs once: the exit codes, the read
// that creates nothing, and the prefix filter.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use nex_fih::{CoordId, ascii};

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_nexus-storage-ref"))
        .args(args)
        .output()
        .expect("the binary runs")
}

fn sample_volume() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("samples/volume")
}

fn volume_arg() -> String {
    sample_volume().to_str().expect("a utf-8 path").to_string()
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "nexus-storage-ref-test-{}-{name}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("a scratch directory");
    dir
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).to_string()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).to_string()
}

fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).expect("a directory");
    for entry in fs::read_dir(from).expect("a directory") {
        let entry = entry.expect("an entry");
        let source = entry.path();
        let target = to.join(entry.file_name());
        if source.is_dir() {
            copy_tree(&source, &target);
        } else {
            fs::copy(&source, &target).expect("a copy");
        }
    }
}

/// The one record the sample volume holds: its address and its byte length.
fn sample_record() -> (String, u64) {
    let entry = fs::read_dir(sample_volume().join("facts"))
        .expect("the facts directory")
        .next()
        .expect("one record")
        .expect("a directory entry");
    let name = entry.file_name().into_string().expect("a utf-8 file name");
    let address = name
        .strip_prefix("f_")
        .and_then(|rest| rest.strip_suffix(".fact"))
        .expect("a record key")
        .to_string();
    let size = entry.metadata().expect("the record's metadata").len();
    (address, size)
}

#[test]
fn a_read_refuses_a_volume_that_does_not_exist_and_creates_nothing() {
    let dir = scratch("missing");
    let volume = dir.join("records");
    let out = run(&["keys", "--volume", volume.to_str().expect("a path")]);

    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    assert!(!volume.exists(), "the read created the volume");
}

#[test]
fn an_address_that_is_not_a_coordinate_is_refused_before_the_volume_is_opened() {
    let dir = scratch("malformed");
    let volume = dir.join("records");
    let out = run(&[
        "read",
        "--volume",
        volume.to_str().expect("a path"),
        "--address",
        "not-an-address",
    ]);

    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    assert!(stderr(&out).contains("is not a record address"));
    assert!(!volume.exists(), "the refused read opened the volume");
}

#[test]
fn an_address_the_volume_does_not_hold_is_absent() {
    let absent = "가".repeat(20);
    let (address, _) = sample_record();
    assert_ne!(absent, address, "the absent address is the fixture's");

    let out = run(&["read", "--volume", &volume_arg(), "--address", &absent]);

    assert_eq!(out.status.code(), Some(2), "{}", stderr(&out));
    assert!(stderr(&out).contains("absent:"));
}

#[test]
fn a_present_address_reads_the_record_s_byte_length() {
    let (address, size) = sample_record();
    let out = run(&["read", "--volume", &volume_arg(), "--address", &address]);

    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let text = stdout(&out);
    assert!(text.contains("present:    yes"), "{text}");
    assert!(text.contains(&format!("bytes:      {size}")), "{text}");
}

#[test]
fn a_coordinate_prefix_finds_the_record_and_a_wrong_one_finds_nothing() {
    let (address, _) = sample_record();
    let first: String = address.chars().take(1).collect();
    let other = if first == "각" { "가" } else { "각" };

    let hit = run(&[
        "keys",
        "--volume",
        &volume_arg(),
        "--prefix",
        &format!("facts/f_{first}"),
    ]);
    assert_eq!(hit.status.code(), Some(0), "{}", stderr(&hit));
    assert!(stdout(&hit).contains("1 key(s)"), "{}", stdout(&hit));

    let miss = run(&[
        "keys",
        "--volume",
        &volume_arg(),
        "--prefix",
        &format!("facts/f_{other}"),
    ]);
    assert_eq!(miss.status.code(), Some(0), "{}", stderr(&miss));
    assert!(stdout(&miss).contains("0 key(s)"), "{}", stdout(&miss));
}

#[test]
fn a_second_placement_is_refused_and_the_app_file_is_unchanged() {
    let dir = scratch("sidecar");
    let app = dir.join("app.db");
    let contents = b"existing application state";
    fs::write(&app, contents).expect("an app file");
    let volume = dir.join("records");

    let first = run(&[
        "sidecar",
        "--app",
        app.to_str().expect("a path"),
        "--volume",
        volume.to_str().expect("a path"),
    ]);
    assert_eq!(first.status.code(), Some(0), "{}", stderr(&first));
    assert!(stdout(&first).contains("unchanged"), "{}", stdout(&first));
    assert_eq!(
        fs::read(&app).expect("the app file"),
        contents,
        "the app file changed"
    );

    let second = run(&[
        "sidecar",
        "--app",
        app.to_str().expect("a path"),
        "--volume",
        volume.to_str().expect("a path"),
    ]);
    assert_eq!(second.status.code(), Some(1), "{}", stderr(&second));
    assert!(stderr(&second).contains("already holds content"));
}

#[test]
fn an_address_in_the_rendering_reads_the_same_record() {
    let (address, size) = sample_record();
    let coordinate = CoordId::<20>::from_string(&address).expect("a coordinate");
    let rendered = ascii::encode(&coordinate);

    let out = run(&["read", "--volume", &volume_arg(), "--address", &rendered]);

    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let text = stdout(&out);
    assert!(text.contains(&format!("bytes:      {size}")), "{text}");
    assert!(
        text.contains(&address),
        "the rendering resolved to another address: {text}"
    );
}

#[test]
fn a_rendering_the_volume_does_not_hold_is_absent() {
    let absent = CoordId::<20>::from_string(&"가".repeat(20)).expect("a coordinate");
    let rendered = ascii::encode(&absent);

    let out = run(&["read", "--volume", &volume_arg(), "--address", &rendered]);

    assert_eq!(out.status.code(), Some(2), "{}", stderr(&out));
    assert!(stderr(&out).contains("absent:"));
}

#[test]
fn a_sixty_character_string_outside_the_alphabet_is_refused() {
    let out = run(&[
        "read",
        "--volume",
        &volume_arg(),
        "--address",
        &"a".repeat(60),
    ]);

    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    assert!(stderr(&out).contains("is not a record address"));
}

#[test]
fn a_usage_error_is_a_refused_question_rather_than_an_absent_record() {
    let out = run(&["read", "--address", "가"]);

    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
}

#[test]
fn help_is_not_a_failure() {
    let out = run(&["--help"]);

    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
}

#[test]
fn a_box_over_one_axis_answers_from_the_keys() {
    let (address, _) = sample_record();
    let axis = address.chars().next().expect("a coordinate");
    let other = if axis == '각' { '가' } else { '각' };

    let hit = run(&[
        "box",
        "--volume",
        &volume_arg(),
        "--axis",
        &format!("0={axis}-{axis}"),
    ]);
    assert_eq!(hit.status.code(), Some(0), "{}", stderr(&hit));
    assert!(stdout(&hit).contains("1 record(s)"), "{}", stdout(&hit));
    assert!(
        stdout(&hit).contains(&address),
        "the box did not name the record: {}",
        stdout(&hit)
    );

    let miss = run(&[
        "box",
        "--volume",
        &volume_arg(),
        "--axis",
        &format!("0={other}-{other}"),
    ]);
    assert_eq!(miss.status.code(), Some(0), "{}", stderr(&miss));
    assert!(stdout(&miss).contains("0 record(s)"), "{}", stdout(&miss));
}

#[test]
fn verifying_the_sample_record_passes() {
    let (address, _) = sample_record();
    let out = run(&["verify", "--volume", &volume_arg(), "--address", &address]);

    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert!(
        stdout(&out).contains("address:    valid"),
        "{}",
        stdout(&out)
    );
}

#[test]
fn a_tampered_payload_fails_verification() {
    let dir = scratch("verify-tampered");
    let volume = dir.join("records");
    copy_tree(&sample_volume(), &volume);

    let blob = fs::read_dir(volume.join("blob"))
        .expect("the blob directory")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| path.extension().is_some_and(|ext| ext == "bin"))
        .expect("a payload");
    let mut payload = fs::read(&blob).expect("the payload");
    payload[0] ^= 0xFF;
    fs::write(&blob, &payload).expect("the tampered payload");

    let (address, _) = sample_record();
    let out = run(&[
        "verify",
        "--volume",
        volume.to_str().expect("a path"),
        "--address",
        &address,
    ]);

    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    assert!(
        stdout(&out).contains("address:    invalid"),
        "{}",
        stdout(&out)
    );
}

#[test]
fn the_self_check_passes_on_the_address_the_volume_holds() {
    let out = run(&["self-check"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));

    let text = stdout(&out);
    assert!(text.contains("self-check: PASS"), "{text}");
    let (address, _) = sample_record();
    assert!(text.contains(&address), "the check used another address");
}
