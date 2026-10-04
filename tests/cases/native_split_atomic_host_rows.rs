//! Split recovery preserves the established prefix/control source rows.
//! The oversized nested table is intentionally outside width-resizing scope.
#![cfg(not(target_arch = "wasm32"))]

use rhwp::document_core::DocumentCore;
use rhwp::model::control::Control;
use rhwp::model::document::{Document, Section, SectionDef};
use rhwp::model::page::PageDef;
use rhwp::model::paragraph::{CharShapeRef, LineSeg, Paragraph};
use rhwp::model::table::{Cell, Table, TablePageBreak};
use rhwp::renderer::render_tree::{RenderNode, RenderNodeType};

fn text(value: &str, width: u32) -> Paragraph {
    Paragraph {
        text: value.into(),
        char_count: value.encode_utf16().count() as u32 + 1,
        char_offsets: (0..value.len() as u32).collect(),
        char_shapes: vec![CharShapeRef {
            start_pos: 0,
            char_shape_id: 0,
        }],
        line_segs: vec![LineSeg {
            line_height: 1000,
            text_height: 1000,
            baseline_distance: 850,
            line_spacing: 600,
            segment_width: width as i32,
            tag: LineSeg::TAG_SINGLE_SEGMENT_LINE,
            ..Default::default()
        }],
        ..Default::default()
    }
}

fn fixture() -> DocumentCore {
    let mut nested = Table {
        attr: 1,
        row_count: 1,
        col_count: 1,
        page_break: TablePageBreak::None,
        outer_margin_top: 150,
        outer_margin_bottom: 150,
        cells: vec![Cell {
            width: 30000,
            height: 4000,
            row_span: 1,
            col_span: 1,
            paragraphs: vec![text("NestedMarker", 30000)],
            ..Default::default()
        }],
        ..Default::default()
    };
    nested.common.width = 30000;
    nested.common.height = 4000;
    nested.common.treat_as_char = true;
    nested.common.flow_with_text = true;
    nested.rebuild_grid();

    let mut host = text("        ", 45000);
    host.char_offsets = vec![0, 1, 2, 3, 4, 5, 14, 15];
    host.char_count = 17;
    host.controls = vec![Control::Table(Box::new(nested))];
    host.line_segs[0].line_height = 4300;
    host.line_segs[0].text_height = 4300;
    host.line_segs[0].baseline_distance = 3655;

    let mut following = text("AfterNested", 45000);
    following.line_segs[0].vertical_pos = 4900;

    let mut outer = Table {
        row_count: 1,
        col_count: 1,
        page_break: TablePageBreak::RowBreak,
        cells: vec![Cell {
            width: 45000,
            height: 20000,
            row_span: 1,
            col_span: 1,
            paragraphs: vec![host, following],
            ..Default::default()
        }],
        ..Default::default()
    };
    outer.common.width = 45000;
    outer.common.height = 20000;
    outer.rebuild_grid();

    let mut body = text("", 45000);
    body.char_count = 9;
    body.controls = vec![Control::Table(Box::new(outer))];

    let mut source = Document::default();
    source.doc_info.char_shapes = vec![rhwp::model::style::CharShape {
        base_size: 1000,
        ..Default::default()
    }];
    source.doc_info.para_shapes = vec![rhwp::model::style::ParaShape {
        line_spacing: 160,
        ..Default::default()
    }];
    source.doc_info.styles = vec![Default::default()];

    let mut page = PageDef::a4_default();
    page.width = 60000;
    page.height = 75000;
    page.margin_left = 7500;
    page.margin_right = 7500;
    page.margin_top = 7500;
    page.margin_bottom = 7500;
    page.margin_header = 0;
    page.margin_footer = 0;
    source.sections.push(Section {
        section_def: SectionDef {
            page_def: page,
            ..Default::default()
        },
        paragraphs: vec![body],
        ..Default::default()
    });

    let bytes = rhwp::serializer::serialize_document(&source).expect("synthetic native HWP");
    DocumentCore::from_bytes(&bytes).expect("public native import")
}

fn table_control_index(core: &DocumentCore) -> usize {
    core.document().sections[0].paragraphs[0]
        .controls
        .iter()
        .position(|control| matches!(control, Control::Table(_)))
        .expect("imported outer table")
}

fn nodes<'a>(node: &'a RenderNode, result: &mut Vec<&'a RenderNode>) {
    result.push(node);
    for child in &node.children {
        nodes(child, result);
    }
}

fn check(core: &DocumentCore) {
    let control_index = table_control_index(core);
    let Control::Table(table) = &core.document().sections[0].paragraphs[0].controls[control_index]
    else {
        panic!("outer table");
    };
    let cell = table
        .cells
        .iter()
        .find(|cell| cell.row == 0 && cell.col == 0)
        .expect("split source cell");
    let host = &cell.paragraphs[0];
    assert_eq!(host.text, "        ", "all source whitespace survives");
    assert_eq!(host.char_offsets, [0, 1, 2, 3, 4, 5, 14, 15]);
    assert_eq!(host.line_segs.len(), 2, "prefix and control source rows");
    assert_eq!(host.line_segs[0].text_start, 0);
    assert_eq!(host.line_segs[1].text_start, 14, "source control boundary");

    for row in &host.line_segs {
        assert_eq!(row.tag & LineSeg::TAG_IMPLEMENTATION_PROPERTY, 0);
        assert!(row.segment_width > 0 && row.segment_width <= cell.width as i32);
    }

    let prefix = &host.line_segs[0];
    let object = &host.line_segs[1];
    assert!(object.vertical_pos >= prefix.vertical_pos + prefix.line_height);
    assert!(
        object.line_height >= 4000,
        "object row retains declared height"
    );
    assert!(
        cell.paragraphs[1].line_segs[0].vertical_pos >= object.vertical_pos + object.line_height,
        "following source row clears the nested object"
    );
    assert_eq!(core.page_count(), 1, "the declared cell fits the page");

    let tree = core
        .build_page_render_tree(0)
        .expect("final split render tree");
    let mut rendered = Vec::new();
    nodes(&tree.root, &mut rendered);
    let frames = rendered
        .iter()
        .filter(|node| {
            matches!(node.node_type, RenderNodeType::Table { .. })
                && (node.bbox.width - 30000.0 / 75.0).abs() < 0.5
        })
        .collect::<Vec<_>>();
    assert_eq!(frames.len(), 1, "declared nested frame appears once");
    assert!(frames[0].bbox.height >= 4000.0 / 75.0 - 0.5);

    for marker in ["NestedMarker", "AfterNested"] {
        let runs = rendered
            .iter()
            .filter(|node| {
                matches!(&node.node_type, RenderNodeType::TextRun(run) if run.text == marker)
            })
            .collect::<Vec<_>>();
        assert_eq!(runs.len(), 1, "each authored marker appears once: {marker}");
        if marker == "AfterNested" {
            assert!(
                runs[0].bbox.y >= frames[0].bbox.y + frames[0].bbox.height - 0.5,
                "following text clears the actual nested frame"
            );
        }
    }
}

#[test]
fn native_atomic_middle_host_keeps_source_rows_after_cell_split() {
    let mut core = fixture();
    let control_index = table_control_index(&core);
    core.split_table_cell_into_native(0, 0, control_index, 0, 0, 1, 2, false, false)
        .expect("public cell split");
    check(&core);

    let bytes = core.export_hwp_native().expect("native split export");
    check(&DocumentCore::from_bytes(&bytes).expect("native split reopen"));
}

#[test]
fn native_atomic_middle_host_keeps_source_rows_after_range_split() {
    let mut core = fixture();
    let control_index = table_control_index(&core);
    core.split_table_cells_in_range_native(0, 0, control_index, 0, 0, 0, 0, 1, 2, false)
        .expect("public range split");
    check(&core);
}
