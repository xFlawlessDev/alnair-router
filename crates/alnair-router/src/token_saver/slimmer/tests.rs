// Tests for the slimmer implementation live alongside detection/filter code.

use super::detect::detect;
use super::{SlimmerLevel, compress_one};

/// Grows a document past the compression size floor without repeating a line,
/// so the dedup fallback does not mask what the probe is measuring.
fn pad(text: &str) -> String {
    let mut out = text.to_string();
    let mut index = 0;
    while out.len() < 600 {
        out.push_str(&format!(
            "\nFiller line {index} keeps the blob above the floor."
        ));
        index += 1;
    }
    out
}

#[test]
fn probe_markdown_source_with_error_line_is_left_alone() {
    let doc = pad(
        "# Fixing builds\n\nWhen a build fails you see:\n\nERROR: cannot find module 'x'\n\nThen rerun.",
    );
    assert_eq!(detect(&doc), None, "reported as {:?}", detect(&doc));
    assert!(compress_one(&doc, SlimmerLevel::Minimal).is_none());
}

#[test]
fn probe_rust_version_mention_with_help_prefix_is_left_alone() {
    let doc =
        pad("Upgrade notes.\n\nhelp: run `cargo update` to refresh the lock file.\n\nThat is all.");
    assert_eq!(detect(&doc), None, "reported as {:?}", detect(&doc));
    assert!(compress_one(&doc, SlimmerLevel::Minimal).is_none());
}

#[test]
fn probe_markdown_file_reference_with_line_number_is_left_alone() {
    let doc = pad("# Guide\n\nSee README.md:42: how it works.\n\nNothing else here.");
    assert_eq!(detect(&doc), None, "reported as {:?}", detect(&doc));
    assert!(compress_one(&doc, SlimmerLevel::Minimal).is_none());
}

#[test]
fn a_long_markdown_document_survives_both_levels_byte_for_byte() {
    let mut doc = String::from("# Long guide\n\nIntro paragraph that sets the scene.\n\n---\n\n");
    for index in 0..140 {
        doc.push_str(&format!(
            "## Section {index}\n\nThis section explains one more part of the tool.\n\n"
        ));
    }

    // Above the truncate floor, so the fallback would have cut the middle.
    assert!(doc.lines().count() > 250);
    for level in SlimmerLevel::ALL {
        assert!(
            compress_one(&doc, level).is_none(),
            "{level:?} must leave the document alone"
        );
    }
}
