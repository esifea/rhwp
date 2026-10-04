//! Fully synthetic source-position restoration and mismatch fallback contracts.

use rhwp::document_core::DocumentCore;
use rhwp::model::document::{Document, Section};
use rhwp::model::paragraph::{CharShapeRef, LineSeg, Paragraph};

const CURRENT_POSITIONS: &[i32] = &[4100, 5200, 6300];

fn assert_snapshot_projection(
    current_positions: &[i32],
    saved_positions: Option<Vec<i32>>,
    layout_only_fill_lines: usize,
    expected_positions: &[i32],
) {
    const TEXT: &str = "Synthetic rows";
    let mut document = Document::default();
    document.doc_info.char_shapes = vec![Default::default()];
    document.doc_info.para_shapes = vec![Default::default()];
    document.doc_info.styles = vec![Default::default()];
    document.sections.push(Section {
        paragraphs: vec![Paragraph {
            text: TEXT.into(),
            char_count: TEXT.encode_utf16().count() as u32 + 1,
            char_offsets: (0..TEXT.encode_utf16().count() as u32).collect(),
            char_shapes: vec![CharShapeRef {
                start_pos: 0,
                char_shape_id: 0,
            }],
            ..Default::default()
        }],
        ..Default::default()
    });
    let mut core = DocumentCore::new_empty();
    core.set_document(document);

    let current_segments: Vec<LineSeg> = current_positions
        .iter()
        .enumerate()
        .map(|(index, position)| LineSeg {
            text_start: index as u32 * 5,
            vertical_pos: *position,
            line_height: 1200,
            text_height: 1100,
            baseline_distance: 810,
            line_spacing: 700,
            column_start: 35,
            segment_width: 14000,
            tag: 0x00060000,
        })
        .collect();

    // Install cache state after layout to isolate snapshot projection.
    let paragraph = &mut core.document_mut().sections[0].paragraphs[0];
    paragraph.line_segs = current_segments.clone();
    paragraph.line_segs_are_layout_only = Some(false);
    paragraph.source_line_seg_vertical_pos = saved_positions.clone();
    paragraph.layout_only_fill_lines = layout_only_fill_lines;

    assert_eq!(
        expected_positions.len(),
        current_positions.len() - layout_only_fill_lines
    );

    let expected_segments: Vec<LineSeg> = current_segments
        .iter()
        .zip(expected_positions)
        .map(|(segment, position)| LineSeg {
            vertical_pos: *position,
            ..segment.clone()
        })
        .collect();
    let before_document = format!("{:?}", core.document());
    let before_pages = core.page_count();
    let assert_projected = |projected: &Document| {
        let paragraph = &projected.sections[0].paragraphs[0];
        assert_eq!(paragraph.line_segs, expected_segments);
        assert_eq!(paragraph.layout_only_fill_lines, 0);
    };

    let source_snapshot = core.source_line_cache_snapshot();
    assert_projected(&source_snapshot);
    assert_eq!(format!("{:?}", core.document()), before_document);
    assert_eq!(core.page_count(), before_pages);

    let export_snapshot = core.prepare_hwp_export_snapshot();
    assert_projected(export_snapshot.document());
    assert_eq!(format!("{:?}", core.document()), before_document);
    assert_eq!(core.page_count(), before_pages);
}

#[test]
fn no_saved_snapshot_retains_current_positions() {
    assert_snapshot_projection(CURRENT_POSITIONS, None, 0, CURRENT_POSITIONS);
}

#[test]
fn equal_length_snapshot_restores_original_positions() {
    assert_snapshot_projection(
        CURRENT_POSITIONS,
        Some(vec![101, 202, 303]),
        0,
        &[101, 202, 303],
    );
}

#[test]
fn shorter_snapshot_retains_all_current_positions_without_panicking() {
    assert_snapshot_projection(
        CURRENT_POSITIONS,
        Some(vec![101, 202]),
        0,
        CURRENT_POSITIONS,
    );
}

#[test]
fn longer_snapshot_retains_all_current_positions_without_panicking() {
    assert_snapshot_projection(
        CURRENT_POSITIONS,
        Some(vec![101, 202, 303, 404]),
        0,
        CURRENT_POSITIONS,
    );
}

#[test]
fn empty_snapshot_retains_all_current_positions_without_panicking() {
    assert_snapshot_projection(CURRENT_POSITIONS, Some(vec![]), 0, CURRENT_POSITIONS);
}

#[test]
fn snapshot_with_no_current_segments_does_not_panic() {
    assert_snapshot_projection(&[], Some(vec![101]), 0, &[]);
}

#[test]
fn equal_snapshot_restores_positions_before_truncating_layout_only_suffix() {
    assert_snapshot_projection(CURRENT_POSITIONS, Some(vec![101, 202, 303]), 1, &[101, 202]);
}

#[test]
fn snapshot_matching_source_prefix_keeps_current_positions_and_truncates_suffix() {
    assert_snapshot_projection(CURRENT_POSITIONS, Some(vec![101, 202]), 1, &[4100, 5200]);
}
