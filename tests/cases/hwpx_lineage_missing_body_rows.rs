//! Synthetic roundtrip contract: HWPX-origin body rows retain their text partitions and metrics.
//! The public 26-paragraph fixture uses only repeated Korean example prose. This tests native
//! persistence consistency, not independent Hancom visual fidelity or fixed pixel positions.

use rhwp::document_core::DocumentCore;
use rhwp::model::control::Control;
use rhwp::model::document::{Document, HWPX_ORIGIN_STREAM_PATH};
use rhwp::model::paragraph::{CharShapeRef, LineSeg, Paragraph};
use rhwp::model::table::{Cell, Table, TablePageBreak};
use rhwp::serializer::hwpx::roundtrip::{diff_documents, strip_hwpx_to_hwp_noise};
use serde_json::{json, Value};

const PUBLIC_HWPX: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/hwpx_lineage_missing_rows/public_cjk_words.hwpx"
));

fn row_geometry(core: &DocumentCore) -> Value {
    json!(core
        .document()
        .sections
        .iter()
        .map(|section| {
            section
                .paragraphs
                .iter()
                .map(|paragraph| {
                    paragraph
                        .line_segs
                        .iter()
                        .map(|line| {
                            json!([
                                paragraph.line_seg_text_start_of(line.text_start),
                                line.vertical_pos,
                                line.line_height,
                                line.text_height,
                                line.baseline_distance,
                                line.line_spacing,
                                line.column_start,
                                line.segment_width
                            ])
                        })
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>())
}

fn text_layout(core: &DocumentCore) -> Vec<Value> {
    (0..core.page_count())
        .map(|page| serde_json::from_str(&core.get_page_text_layout_native(page).unwrap()).unwrap())
        .collect()
}

fn lowered_public_document() -> Document {
    let source = DocumentCore::from_bytes(PUBLIC_HWPX).unwrap();
    source.prepare_hwp_export_snapshot().document().clone()
}

fn load_hwp(document: &Document) -> DocumentCore {
    DocumentCore::from_bytes(&rhwp::serializer::serialize_document(document).unwrap()).unwrap()
}

#[test]
fn public_missing_body_rows_preserve_pages_geometry_and_content_across_two_hwp_saves() {
    let source = DocumentCore::from_bytes(PUBLIC_HWPX).unwrap();
    assert_eq!(source.document().sections[0].paragraphs.len(), 26);
    assert!(source.document().sections[0]
        .paragraphs
        .iter()
        .all(|paragraph| {
            !paragraph.line_segs.is_empty() && paragraph.serializable_line_segs().is_empty()
        }));
    let snapshot = source.prepare_hwp_export_snapshot();
    assert!(snapshot.document().sections[0]
        .paragraphs
        .iter()
        .all(|paragraph| paragraph.line_segs.is_empty()));
    let first = DocumentCore::from_bytes(&snapshot.serialize().unwrap()).unwrap();
    let difference = strip_hwpx_to_hwp_noise(diff_documents(
        snapshot.document(),
        &first.source_line_cache_snapshot(),
    ));
    assert!(difference.is_empty(), "{difference:?}");
    assert_eq!(source.page_count(), first.page_count());
    assert_eq!(row_geometry(&source), row_geometry(&first));
    assert_eq!(text_layout(&source), text_layout(&first));

    let second_snapshot = first.prepare_hwp_export_snapshot();
    let second = DocumentCore::from_bytes(&second_snapshot.serialize().unwrap()).unwrap();
    let difference = diff_documents(
        second_snapshot.document(),
        &second.source_line_cache_snapshot(),
    );
    assert!(difference.is_empty(), "{difference:?}");
    assert_eq!(first.page_count(), second.page_count());
    assert_eq!(row_geometry(&first), row_geometry(&second));
    assert_eq!(text_layout(&first), text_layout(&second));
}

#[test]
fn native_hwp_without_hwpx_lineage_keeps_its_missing_row_contract() {
    let mut document = lowered_public_document();
    document
        .extra_streams
        .retain(|(path, _)| path != HWPX_ORIGIN_STREAM_PATH);
    let native = load_hwp(&document);
    assert!(!native.document().provenance.hwpx_lineage);
    assert!(native.document().sections[0]
        .paragraphs
        .iter()
        .all(|paragraph| paragraph.line_segs.is_empty()));
}

#[test]
fn lineage_preserves_authoritative_saved_rows() {
    let mut document = lowered_public_document();
    document.sections[0].paragraphs.truncate(1);
    let stored = LineSeg {
        text_start: 0,
        vertical_pos: 0,
        line_height: 1200,
        text_height: 1200,
        baseline_distance: 900,
        line_spacing: 600,
        segment_width: 45000,
        tag: LineSeg::TAG_SINGLE_SEGMENT_LINE,
        ..Default::default()
    };
    document.sections[0].paragraphs[0].line_segs = vec![stored.clone()];
    document.sections[0].paragraphs[0].line_segs_are_layout_only = Some(false);
    let loaded = load_hwp(&document);
    assert!(loaded.document().provenance.hwpx_lineage);
    assert_eq!(
        serde_json::to_value(loaded.document().sections[0].paragraphs[0].serializable_line_segs())
            .unwrap(),
        json!([stored]),
    );
}

#[test]
fn lineage_leaves_non_neutral_picture_hosts_with_their_existing_owner() {
    let mut document = lowered_public_document();
    document.sections[0].paragraphs.truncate(1);
    let mut picture = rhwp::model::image::Picture::default();
    picture.common.width = 7200;
    picture.common.height = 3600;
    picture.common.treat_as_char = true;
    document.sections[0].paragraphs[0]
        .controls
        .push(Control::Picture(Box::new(picture)));
    let loaded = load_hwp(&document);
    assert!(loaded.document().sections[0].paragraphs[0]
        .line_segs
        .is_empty());
    assert!(loaded.document().sections[0].paragraphs[0]
        .controls
        .iter()
        .any(|control| matches!(control, Control::Picture(_))));
}

#[test]
fn lineage_retains_cache_free_tac_host_reflow_without_expanding_text_cell_admission() {
    let mut document = lowered_public_document();
    document.sections[0].paragraphs.truncate(1);
    let mut table = Table {
        row_count: 1,
        col_count: 1,
        page_break: TablePageBreak::None,
        cells: vec![Cell {
            width: 20000,
            height: 3600,
            row_span: 1,
            col_span: 1,
            paragraphs: vec![Paragraph {
                text: "public cell".into(),
                char_count: 12,
                char_offsets: (0..11).collect(),
                char_shapes: vec![CharShapeRef {
                    start_pos: 0,
                    char_shape_id: 0,
                }],
                ..Default::default()
            }],
            ..Default::default()
        }],
        ..Default::default()
    };
    table.common.width = 20000;
    table.common.height = 3600;
    table.common.treat_as_char = true;
    table.common.flow_with_text = true;
    table.rebuild_grid();
    document.sections[0].paragraphs[0]
        .controls
        .push(Control::Table(Box::new(table)));
    rhwp::document_core::converters::hwpx_to_hwp::convert_hwpx_to_hwp_ir(&mut document);
    let loaded = load_hwp(&document);
    let host = &loaded.document().sections[0].paragraphs[0];
    assert!(!host.line_segs.is_empty());
    assert!(host.serializable_line_segs().is_empty());
    let cell = host
        .controls
        .iter()
        .find_map(|control| match control {
            Control::Table(table) => Some(&table.cells[0].paragraphs[0]),
            _ => None,
        })
        .unwrap();
    assert_eq!(cell.text, "public cell");
    assert!(cell.line_segs.is_empty());
}
