//! Missing source caches must stay missing through layout, HWP save and HWPX export.
#![cfg(not(target_arch = "wasm32"))]

use rhwp::document_core::DocumentCore;
use rhwp::model::control::{Control, Field, FieldType};
use rhwp::model::document::{Document, Section};
use rhwp::model::paragraph::{CharShapeRef, FieldRange, LineSeg, Paragraph};
use rhwp::model::table::{Cell, Table};
use rhwp::serializer::hwpx::roundtrip::{
    diff_documents, strip_hwp_to_hwpx_noise, strip_hwpx_to_hwp_noise,
};

fn paragraph(text: &str) -> Paragraph {
    Paragraph {
        text: text.into(),
        char_count: text.encode_utf16().count() as u32 + 1,
        char_offsets: (0..text.encode_utf16().count() as u32).collect(),
        char_shapes: vec![CharShapeRef {
            start_pos: 0,
            char_shape_id: 0,
        }],
        ..Default::default()
    }
}

fn document(paragraphs: Vec<Paragraph>) -> Document {
    let mut doc = Document::default();
    doc.doc_info.char_shapes = vec![Default::default()];
    doc.doc_info.para_shapes = vec![Default::default()];
    doc.doc_info.styles = vec![Default::default()];
    doc.sections.push(Section {
        paragraphs,
        ..Default::default()
    });
    doc
}

fn table(paragraphs: Vec<Paragraph>, merged: bool) -> Table {
    let mut table = Table {
        row_count: 1,
        col_count: if merged { 2 } else { 1 },
        cells: vec![Cell {
            row: 0,
            col: 0,
            row_span: 1,
            col_span: if merged { 2 } else { 1 },
            width: 14000,
            height: 6000,
            paragraphs,
            ..Default::default()
        }],
        ..Default::default()
    };
    table.common.width = 14000;
    table.common.height = 6000;
    table.rebuild_grid();
    table
}

fn fixture(blank_cell: bool) -> Vec<u8> {
    let mut annotated = paragraph("Merged OLD value");
    annotated.controls.push(Control::Field(Field {
        field_type: FieldType::Memo,
        ctrl_id: rhwp::parser::tags::FIELD_MEMO,
        field_id: 101,
        memo_index: 1,
        command: r"MEMO/65535/1/123/456/Synthetic Author/\;;".into(),
        memo_paragraphs: vec![paragraph("Synthetic note")],
        ..Default::default()
    }));
    annotated.field_ranges.push(FieldRange {
        start_char_idx: 0,
        end_char_idx: annotated.text.chars().count(),
        control_idx: 0,
        ..Default::default()
    });
    let inner = table(vec![paragraph("Nested OLD value")], false);
    let mut nested = paragraph("");
    nested.controls.push(Control::Table(Box::new(inner)));
    let mut cells = vec![annotated, nested];
    if blank_cell {
        cells.push(paragraph(""));
    }
    let mut host = paragraph("");
    host.controls
        .push(Control::Table(Box::new(table(cells, true))));
    let doc = document(vec![
        paragraph("Body OLD value"),
        host,
        paragraph("After table"),
    ]);
    rhwp::serializer::serialize_hwpx(&doc).expect("author cache-free HWPX")
}

fn rows(doc: &Document) -> Vec<Vec<LineSeg>> {
    fn visit(paragraphs: &[Paragraph], out: &mut Vec<Vec<LineSeg>>) {
        for para in paragraphs {
            out.push(para.line_segs.clone());
            for control in &para.controls {
                match control {
                    Control::Table(table) => {
                        for cell in &table.cells {
                            visit(&cell.paragraphs, out);
                        }
                    }
                    Control::Field(field) => visit(&field.memo_paragraphs, out),
                    _ => {}
                }
            }
        }
    }
    let mut out = Vec::new();
    for section in &doc.sections {
        visit(&section.paragraphs, &mut out);
    }
    out
}

fn gated_cycle(core: &DocumentCore) -> Vec<u8> {
    let live_rows = rows(core.document());
    let pages = core.page_count();
    core.render_page_svg_native(0)
        .expect("render before persistence");
    let snapshot = core.prepare_hwp_export_snapshot();
    let hwp = snapshot.serialize().expect("serialize HWP snapshot");
    assert_eq!(
        live_rows,
        rows(core.document()),
        "save cannot alter live layout"
    );
    let raw = rhwp::parse_document(&hwp).expect("parse stored HWP source");
    assert!(
        rows(&raw).iter().all(Vec::is_empty),
        "generated rows became stored source caches"
    );
    let loaded = DocumentCore::from_bytes(&hwp).expect("reopen HWP through CLI API");
    assert_eq!(pages, loaded.page_count(), "HWP page gate");
    let loaded_source = loaded.source_line_cache_snapshot();
    let diff = strip_hwpx_to_hwp_noise(diff_documents(snapshot.document(), &loaded_source));
    assert!(
        diff.is_empty(),
        "HWP strict IR gate: {:?}",
        diff.differences
    );
    let hwpx = loaded
        .export_hwpx_native()
        .expect("export HWPX through CLI API");
    let normalized = DocumentCore::from_bytes(&hwpx).expect("reopen normalized HWPX");
    assert_eq!(pages, normalized.page_count(), "HWPX page gate");
    let diff = strip_hwp_to_hwpx_noise(diff_documents(
        &loaded.source_line_cache_snapshot(),
        &normalized.source_line_cache_snapshot(),
    ));
    assert!(
        diff.is_empty(),
        "HWPX strict IR gate: {:?}",
        diff.differences
    );
    let raw = rhwp::parse_document(&hwpx).expect("parse stored HWPX source");
    assert!(
        rows(&raw).iter().all(Vec::is_empty),
        "missing source cache must stay missing"
    );
    hwpx
}

#[test]
fn cache_free_nested_merged_memo_document_reexports_without_new_source_rows() {
    let source = fixture(false);
    assert!(rows(&rhwp::parse_document(&source).unwrap())
        .iter()
        .all(Vec::is_empty));
    let core = DocumentCore::from_bytes(&source).expect("load source");
    assert!(
        rows(core.document()).iter().any(|rows| !rows.is_empty()),
        "layout must actually generate rows"
    );
    let output = gated_cycle(&core);
    let parsed = rhwp::parse_document(&output).unwrap();
    let Control::Table(table) = &parsed.sections[0].paragraphs[1].controls[0] else {
        panic!("table");
    };
    assert_eq!(table.cells[0].col_span, 2);
    let Control::Field(memo) = &table.cells[0].paragraphs[0].controls[0] else {
        panic!("memo");
    };
    assert_eq!(memo.memo_paragraphs[0].text, "Synthetic note");
    assert_eq!(parsed.sections[0].paragraphs[2].text, "After table");
}

#[test]
fn repeated_visible_edits_and_export_keep_missing_cache_contract() {
    let mut source = fixture(false);
    for _ in 0..2 {
        let mut core = DocumentCore::from_bytes(&source).expect("load each edit cycle");
        core.insert_text_native(0, 0, 0, "New ").expect("body edit");
        core.insert_text_in_cell_by_path(0, 1, &[(0, 0, 1), (0, 0, 0)], 0, "New ")
            .expect("nested visible cell edit");
        source = gated_cycle(&core);
    }
    let parsed = rhwp::parse_document(&source).unwrap();
    assert_eq!(
        parsed.sections[0].paragraphs[0].text,
        "New New Body OLD value"
    );
    let Control::Table(outer) = &parsed.sections[0].paragraphs[1].controls[0] else {
        panic!("outer");
    };
    let Control::Table(inner) = &outer.cells[0].paragraphs[1].controls[0] else {
        panic!("inner");
    };
    assert_eq!(
        inner.cells[0].paragraphs[0].text,
        "New New Nested OLD value"
    );
}

#[test]
fn empty_cell_reimport_obeys_both_strict_gates() {
    let core = DocumentCore::from_bytes(&fixture(true)).expect("load empty-cell source");
    gated_cycle(&core);
}

#[test]
fn paragraph_split_and_merge_do_not_promote_generated_rows() {
    let source =
        rhwp::serializer::serialize_hwpx(&document(vec![paragraph("Split OLD value")])).unwrap();
    let core = DocumentCore::from_bytes(&source).unwrap();
    let mut first = core.document().sections[0].paragraphs[0].clone();
    assert!(!first.line_segs.is_empty());
    let second = first.split_at(6);
    let section = Section {
        paragraphs: vec![first.clone(), second.clone()],
        ..Default::default()
    };
    let parsed = rhwp::parser::body_text::parse_body_text_section(
        &rhwp::serializer::body_text::serialize_section(&section),
    )
    .unwrap();
    assert!(
        parsed
            .paragraphs
            .iter()
            .all(|para| para.line_segs.is_empty()),
        "split promoted generated rows"
    );
    first.merge_from(&second);
    let section = Section {
        paragraphs: vec![first],
        ..Default::default()
    };
    let parsed = rhwp::parser::body_text::parse_body_text_section(
        &rhwp::serializer::body_text::serialize_section(&section),
    )
    .unwrap();
    assert!(
        parsed.paragraphs[0].line_segs.is_empty(),
        "merge promoted generated rows"
    );
}

#[test]
fn table_merge_copies_generated_provenance_and_keeps_source_prefix() {
    let source =
        rhwp::serializer::serialize_hwpx(&document(vec![paragraph("Copied OLD value")])).unwrap();
    let core = DocumentCore::from_bytes(&source).unwrap();
    let generated = core.document().sections[0].paragraphs[0].clone();
    let mut source_para = paragraph("source");
    source_para.line_segs = vec![
        LineSeg {
            text_start: 0,
            line_height: 1000,
            tag: 0x60000,
            ..Default::default()
        },
        LineSeg {
            text_start: 1,
            line_height: 1000,
            tag: 0x80060000,
            ..Default::default()
        },
    ];
    source_para.layout_only_fill_lines = 1;
    let source_prefix = source_para.line_segs[0].clone();
    let mut table = Table {
        row_count: 1,
        col_count: 2,
        cells: vec![
            Cell {
                row: 0,
                col: 0,
                row_span: 1,
                col_span: 1,
                paragraphs: vec![source_para],
                ..Default::default()
            },
            Cell {
                row: 0,
                col: 1,
                row_span: 1,
                col_span: 1,
                paragraphs: vec![generated],
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    table.rebuild_grid();
    table.merge_cells(0, 0, 0, 1).unwrap();
    let section = Section {
        paragraphs: table.cells[0].paragraphs.clone(),
        ..Default::default()
    };
    let parsed = rhwp::parser::body_text::parse_body_text_section(
        &rhwp::serializer::body_text::serialize_section(&section),
    )
    .unwrap();
    assert_eq!(parsed.paragraphs[0].line_segs, vec![source_prefix]);
    assert!(
        parsed.paragraphs[1].line_segs.is_empty(),
        "table merge promoted generated rows"
    );
}

#[test]
fn mixed_source_prefix_and_layout_suffix_share_snapshot_and_strict_gate_view() {
    let source_row = LineSeg {
        text_start: 0,
        vertical_pos: 100,
        line_height: 1200,
        text_height: 1100,
        baseline_distance: 810,
        line_spacing: 700,
        column_start: 35,
        segment_width: 14000,
        tag: 0x80060000,
    };
    let mut para = paragraph("Native source");
    para.line_segs = vec![source_row.clone()];
    let bytes = rhwp::serializer::serialize_hwp(&document(vec![para])).unwrap();
    let mut source = rhwp::parse_document(&bytes).unwrap();
    assert_eq!(
        source.sections[0].paragraphs[0].line_segs,
        vec![source_row.clone()]
    );
    source.sections[0].paragraphs[0].line_segs.push(LineSeg {
        text_start: 1,
        vertical_pos: 2000,
        ..source_row.clone()
    });
    source.sections[0].paragraphs[0].layout_only_fill_lines = 1;
    let mut core = DocumentCore::new_empty();
    core.set_document(source);
    let before = core.document().sections[0].paragraphs[0].clone();
    let pages = core.page_count();
    assert_eq!(before.line_segs.len(), 2);
    assert_eq!(before.layout_only_fill_lines, 1);
    let snapshot = core.prepare_hwp_export_snapshot();
    let projected = &snapshot.document().sections[0].paragraphs[0];
    assert_eq!(projected.line_segs, vec![source_row.clone()]);
    assert_eq!(projected.layout_only_fill_lines, 0);
    let saved = snapshot.serialize().unwrap();
    let parsed = rhwp::parse_document(&saved).unwrap();
    assert_eq!(
        parsed.sections[0].paragraphs[0].line_segs,
        vec![source_row.clone()]
    );
    let loaded = DocumentCore::from_bytes(&saved).unwrap();
    assert_eq!(loaded.page_count(), pages);
    let diff = diff_documents(snapshot.document(), loaded.document());
    assert!(
        diff.is_empty(),
        "strict native HWP gate: {:?}",
        diff.differences
    );
    assert_eq!(
        core.document().sections[0].paragraphs[0].line_segs,
        before.line_segs
    );
    assert_eq!(
        core.document().sections[0].paragraphs[0].layout_only_fill_lines,
        1
    );
    assert_eq!(core.document().sections[0].paragraphs[0].text, before.text);
    assert_eq!(core.page_count(), pages);
    assert_eq!(
        loaded.document().sections[0].paragraphs[0].line_segs,
        vec![source_row]
    );
}

#[test]
fn genuinely_loaded_native_bit31_line_metrics_remain_exact() {
    let mut para = paragraph("Native source");
    para.line_segs = vec![
        LineSeg {
            text_start: 0,
            vertical_pos: 100,
            line_height: 1200,
            text_height: 1100,
            baseline_distance: 810,
            line_spacing: 700,
            column_start: 35,
            segment_width: 14000,
            tag: 0x80060000,
        },
        LineSeg {
            text_start: 7,
            vertical_pos: 2000,
            line_height: 1300,
            text_height: 1200,
            baseline_distance: 900,
            line_spacing: 750,
            column_start: 45,
            segment_width: 13000,
            tag: 0x80060000,
        },
    ];
    let expected = para.line_segs.clone();
    let bytes = rhwp::serializer::serialize_hwp(&document(vec![para])).unwrap();
    let parsed = rhwp::parse_document(&bytes).unwrap();
    assert_eq!(parsed.sections[0].paragraphs[0].line_segs, expected);
    let core = DocumentCore::from_bytes(&bytes).unwrap();
    let before = rows(core.document());
    let saved = core.prepare_hwp_export_snapshot().serialize().unwrap();
    assert_eq!(rows(core.document()), before);
    let parsed = rhwp::parse_document(&saved).unwrap();
    assert_eq!(
        parsed.sections[0].paragraphs[0].line_segs, expected,
        "stored bit31 is not generated provenance"
    );
}

#[test]
fn stored_line_count_asymmetry_remains_a_strict_failure() {
    let a = document(vec![paragraph("source")]);
    let mut b = a.clone();
    b.sections[0].paragraphs[0].line_segs = vec![LineSeg {
        text_start: 0,
        line_height: 1000,
        tag: 0x60000,
        ..Default::default()
    }];
    let diff = diff_documents(&a, &b);
    assert!(
        !diff.is_empty(),
        "missing versus stored source rows must remain checked"
    );
    let mut left = DocumentCore::new_empty();
    left.set_document(a);
    let mut right = DocumentCore::new_empty();
    right.set_document(b);
    let diff = diff_documents(
        &left.source_line_cache_snapshot(),
        &right.source_line_cache_snapshot(),
    );
    assert!(
        !diff.is_empty(),
        "source-cache projection must retain stored-row asymmetry"
    );
}
