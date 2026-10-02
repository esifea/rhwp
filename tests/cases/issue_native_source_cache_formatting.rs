//! Native formatting must retain source ownership through clear and reflow.
#![cfg(not(target_arch = "wasm32"))]

use rhwp::document_core::DocumentCore;
use rhwp::model::control::Control;
use rhwp::model::document::{Document, Section, SectionDef};
use rhwp::model::page::{ColumnDef, PageDef};
use rhwp::model::paragraph::{CharShapeRef, LineSeg, Paragraph};

fn native_source() -> DocumentCore {
    let text = "Native source formatting control";
    let mut document = Document::default();
    document.doc_info.char_shapes = vec![Default::default()];
    document.doc_info.para_shapes = vec![Default::default()];
    document.doc_info.styles = vec![Default::default()];
    document.sections.push(Section {
        section_def: SectionDef {
            page_def: PageDef {
                width: 59528,
                height: 84188,
                margin_left: 8504,
                margin_right: 8504,
                margin_top: 5669,
                margin_bottom: 5669,
                margin_header: 4252,
                margin_footer: 4252,
                ..Default::default()
            },
            ..Default::default()
        },
        paragraphs: vec![Paragraph {
            text: text.into(),
            char_count: text.encode_utf16().count() as u32 + 1,
            char_offsets: (0..text.encode_utf16().count() as u32).collect(),
            char_shapes: vec![CharShapeRef {
                start_pos: 0,
                char_shape_id: 0,
            }],
            controls: vec![Control::ColumnDef(ColumnDef {
                column_count: 1,
                same_width: true,
                ..Default::default()
            })],
            line_segs: vec![LineSeg {
                text_start: 0,
                line_height: 1000,
                text_height: 1000,
                baseline_distance: 850,
                line_spacing: 600,
                segment_width: 40000,
                tag: 0x80060000,
                ..Default::default()
            }],
            ..Default::default()
        }],
        ..Default::default()
    });
    let bytes = rhwp::serializer::serialize_hwp(&document).expect("author native source metrics");
    let parsed = rhwp::parse_document(&bytes).expect("parse actual native source");
    assert!(
        parsed.sections[0].paragraphs[0]
            .controls
            .iter()
            .any(|control| matches!(control, Control::SectionDef(_))),
        "native fixture must serialize a real section/page definition"
    );
    assert_eq!(
        serde_json::to_value(&parsed.sections[0].section_def.page_def).unwrap(),
        serde_json::to_value(&document.sections[0].section_def.page_def).unwrap(),
        "native fixture page definition must survive initial authoring"
    );
    assert_eq!(
        parsed.sections[0].paragraphs[0].line_segs,
        document.sections[0].paragraphs[0].line_segs
    );
    DocumentCore::from_bytes(&bytes).expect("load native source owner")
}

fn assert_native_persistence(core: &DocumentCore) {
    let owner = &core.document().sections[0].paragraphs[0];
    assert!(
        !owner.line_segs.is_empty(),
        "native edit must actually reflow owner rows"
    );
    assert_eq!(
        owner.serializable_line_segs(),
        owner.line_segs.as_slice(),
        "native formatting cannot relabel source-backed owner as layout only"
    );
    let before = owner.clone();
    let saved = core.export_hwp_native().expect("save native formatting result");
    let raw = rhwp::parse_document(&saved).expect("parse persisted native formatting");
    assert_eq!(
        serde_json::to_value(&raw.sections[0].section_def.page_def).unwrap(),
        serde_json::to_value(&core.document().sections[0].section_def.page_def).unwrap(),
        "all edited page settings must persist before geometry is compared"
    );
    assert_eq!(
        raw.sections[0].paragraphs[0].line_segs, before.line_segs,
        "all native replacement row metrics must persist"
    );
    let loaded = DocumentCore::from_bytes(&saved).expect("reload native formatting");
    let after = &loaded.document().sections[0].paragraphs[0];
    assert_eq!(after.text, before.text);
    assert_eq!(after.char_offsets, before.char_offsets);
    assert_eq!(after.char_count, before.char_count);
    assert_eq!(
        after.char_shapes.iter().map(|s| (s.start_pos, s.char_shape_id)).collect::<Vec<_>>(),
        before.char_shapes.iter().map(|s| (s.start_pos, s.char_shape_id)).collect::<Vec<_>>()
    );
    assert_eq!(
        loaded.page_count(),
        core.page_count(),
        "native formatting page persistence"
    );
    for page in 0..core.page_count() {
        let expected: serde_json::Value = serde_json::from_str(
            &core.get_page_text_layout_native(page).expect("native edited paint layout"),
        )
        .unwrap();
        let actual: serde_json::Value = serde_json::from_str(
            &loaded.get_page_text_layout_native(page).expect("native reloaded paint layout"),
        )
        .unwrap();
        assert_eq!(
            actual, expected,
            "native formatting exact painted text and geometry"
        );
    }
    assert_eq!(
        core.document().sections[0].paragraphs[0].line_segs,
        before.line_segs,
        "save cannot change live owner metrics"
    );
}

#[test]
fn native_apply_style_keeps_source_rows_through_reflow_and_save() {
    let mut core = native_source();
    core.apply_style_native(0, 0, 0).expect("apply native style");
    assert_native_persistence(&core);
}

#[test]
fn native_page_and_column_changes_keep_source_rows_through_section_reflow() {
    let mut core = native_source();
    core.set_page_def_native(0, r#"{"marginLeft":9000}"#).expect("native page width change");
    assert_native_persistence(&core);
    core.set_column_def_native(0, 2, 0, true, 1000).expect("native column width change");
    assert_native_persistence(&core);
}

#[test]
fn unusable_style_box_preserves_metadata_until_successful_reflow() {
    let mut core = native_source();
    let mut document = core.document().clone();
    document.doc_info.para_shapes[0].margin_left = 1_000_000;
    document.doc_info.para_shapes[0].margin_right = 1_000_000;
    let owner = &mut document.sections[0].paragraphs[0];
    owner.line_segs.push(LineSeg {
        text_start: 1,
        ..owner.line_segs[0].clone()
    });
    owner.layout_only_fill_lines = 1;
    owner.source_line_seg_vertical_pos = Some(vec![77, 999]);
    owner.hwpx_axis_shift = 13;
    owner.invalidate_layout_inputs();
    core.set_document(document);
    let before = core.document().sections[0].paragraphs[0].clone();
    assert_eq!(before.layout_only_fill_lines, 1);
    assert_eq!(before.source_line_seg_vertical_pos, Some(vec![77, 999]));
    assert_eq!(before.hwpx_axis_shift, 13);
    assert!(before.stored_text_partition_is_dirty());
    core.apply_style_native(0, 0, 0).expect("apply style with unusable paragraph box");
    let after = &core.document().sections[0].paragraphs[0];
    assert!(
        after.line_segs.is_empty(),
        "original invalidation still clears unusable owner rows"
    );
    assert_eq!(after.layout_only_fill_lines, before.layout_only_fill_lines);
    assert_eq!(
        after.source_line_seg_vertical_pos,
        before.source_line_seg_vertical_pos
    );
    assert_eq!(after.hwpx_axis_shift, before.hwpx_axis_shift);
    assert_eq!(
        after.stored_text_partition_is_dirty(),
        before.stored_text_partition_is_dirty()
    );
}
