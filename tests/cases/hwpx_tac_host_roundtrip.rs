//! Synthetic contract: two full-width TAC objects own separate fresh rows.
//! Bounds come from declared cell heights and the input page body, not reflow.
#![cfg(not(target_arch = "wasm32"))]

use rhwp::document_core::DocumentCore;
use rhwp::model::control::Control;
use rhwp::model::document::{Document, Section, SectionDef};
use rhwp::model::page::PageDef;
use rhwp::model::paragraph::{CharShapeRef, LineSeg, Paragraph};
use rhwp::model::table::{Cell, Table, TablePageBreak};
use rhwp::renderer::render_tree::{RenderNode, RenderNodeType};

const WIDTH: u32 = 45000;
const HEIGHTS: [u32; 2] = [7200, 37400];
const MARGIN: i16 = 150;
const AFTER: &str = "Following body marker";

fn text(text: &str, cached: bool) -> Paragraph {
    Paragraph {
        text: text.into(),
        char_count: text.encode_utf16().count() as u32 + 1,
        char_offsets: (0..text.len() as u32).collect(),
        char_shapes: vec![CharShapeRef {
            start_pos: 0,
            char_shape_id: 0,
        }],
        line_segs: if cached {
            vec![LineSeg {
                line_height: 1000,
                text_height: 1000,
                baseline_distance: 850,
                line_spacing: 600,
                segment_width: WIDTH as i32,
                ..Default::default()
            }]
        } else {
            Vec::new()
        },
        ..Default::default()
    }
}

fn table(rows: u16, row_height: u32, label: &str) -> Table {
    let mut table = Table {
        row_count: rows,
        col_count: 2,
        page_break: TablePageBreak::None,
        outer_margin_top: MARGIN,
        outer_margin_bottom: MARGIN,
        ..Default::default()
    };
    table.common.width = WIDTH;
    table.common.height = u32::from(rows) * row_height;
    table.common.treat_as_char = true;
    table.common.flow_with_text = true;
    table.common.text_wrap = rhwp::model::shape::TextWrap::TopAndBottom;
    table.common.vert_rel_to = rhwp::model::shape::VertRelTo::Para;
    table.common.margin.top = MARGIN;
    table.common.margin.bottom = MARGIN;

    for row in 0..rows {
        for col in 0..2 {
            table.cells.push(Cell {
                row,
                col,
                width: WIDTH / 2,
                height: row_height,
                row_span: 1,
                col_span: 1,
                paragraphs: vec![text(&format!("{label}{row}_{col}"), true)],
                ..Default::default()
            });
        }
    }
    table.rebuild_grid();
    table
}

fn fixture() -> Document {
    let mut host = text("  ", false);
    host.char_offsets = vec![8, 9];
    host.char_count = 19;
    host.controls = vec![
        Control::Table(Box::new(table(4, 1800, "First"))),
        Control::Table(Box::new(table(17, 2200, "Second"))),
    ];
    let mut document = Document::default();
    document.doc_info.char_shapes = vec![rhwp::model::style::CharShape {
        base_size: 1000,
        ..Default::default()
    }];
    document.doc_info.para_shapes = vec![rhwp::model::style::ParaShape {
        line_spacing: 160,
        ..Default::default()
    }];
    document.doc_info.styles = vec![Default::default()];
    let mut page = PageDef::a4_default();
    page.width = 60000;
    page.height = 75000;
    page.margin_left = 7500;
    page.margin_right = 7500;
    page.margin_top = 7500;
    page.margin_bottom = 7500;
    page.margin_header = 0;
    page.margin_footer = 0;
    document.sections.push(Section {
        section_def: SectionDef {
            page_def: page,
            ..Default::default()
        },
        paragraphs: vec![text("Before body marker", true), host, text(AFTER, true)],
        ..Default::default()
    });
    document
}

fn nodes<'a>(node: &'a RenderNode, output: &mut Vec<&'a RenderNode>) {
    output.push(node);
    for child in &node.children {
        nodes(child, output);
    }
}

fn geometry(core: &DocumentCore, source: &Document) -> (Vec<(f64, f64, f64, f64)>, (f64, f64)) {
    assert_eq!(
        core.page_count(),
        1,
        "declared rows and following text fit the 60000 HU body"
    );

    let tree = core.build_page_render_tree(0).expect("actual render tree");
    let mut rendered = Vec::new();
    nodes(&tree.root, &mut rendered);
    let page = &source.sections[0].section_def.page_def;
    let px = |units: u32| f64::from(units) / 75.0;
    let bottom = px(page.height - page.margin_bottom);
    let right = px(page.width - page.margin_right);
    let mut tables = Vec::new();
    let mut following = Vec::new();
    let mut actual_text = String::new();

    for node in rendered {
        match &node.node_type {
            RenderNodeType::Table(_) => {
                let b = &node.bbox;
                eprintln!(
                    "table box={b:?}; input page={page:?}; parsed page={:?}",
                    core.document().sections[0].section_def.page_def
                );
                assert!(
                    b.y >= px(page.margin_top) - 0.5 && b.y + b.height <= bottom + 0.5,
                    "table {b:?} outside vertical body {}..{bottom}",
                    px(page.margin_top)
                );
                assert!(
                    b.x >= px(page.margin_left) - 0.5 && b.x + b.width <= right + 0.5,
                    "table {b:?} outside horizontal body {}..{right}",
                    px(page.margin_left)
                );
                tables.push((b.x, b.y, b.width, b.height));
            }
            RenderNodeType::TextRun(run) => {
                actual_text.push_str(&run.text);
                if run.text.contains(AFTER) {
                    following.push((node.bbox.y, node.bbox.height));
                }
            }
            _ => {}
        }
    }
    assert_eq!(tables.len(), 2, "each NONE table appears once");

    tables.sort_by(|a, b| a.1.total_cmp(&b.1));
    for (bounds, height) in tables.iter().zip(HEIGHTS) {
        assert!((bounds.2 - px(WIDTH)).abs() < 0.5, "declared table width");
        assert!(
            (bounds.3 - px(height)).abs() < 0.5,
            "sum of input cell row heights: {bounds:?}"
        );
    }
    assert!(
        tables[1].1 >= tables[0].1 + tables[0].3 - 0.5,
        "ordered tables cannot overlap"
    );
    assert_eq!(following.len(), 1, "following text appears exactly once");
    assert!(
        following[0].0 >= tables[1].1 + tables[1].3 - 0.5,
        "following text clears table"
    );
    assert!(
        following[0].0 + following[0].1 <= bottom + 0.5,
        "following text fits body"
    );

    let mut expected_text = String::from("Before body marker  ");
    for control in &source.sections[0].paragraphs[1].controls {
        if let Control::Table(table) = control {
            for cell in &table.cells {
                expected_text.push_str(&cell.paragraphs[0].text);
            }
        }
    }

    expected_text.push_str(AFTER);
    let units = |text: &str| {
        let mut chars = text
            .chars()
            .filter(|ch| !ch.is_whitespace())
            .collect::<Vec<_>>();
        chars.sort_unstable();
        chars
    };

    assert_eq!(
        units(&actual_text),
        units(&expected_text),
        "all body and cell text survives exactly once"
    );
    (tables, following[0])
}

#[test]
fn cache_free_full_width_tac_tables_roundtrip_in_their_own_line_boxes() {
    let source = fixture();
    let bytes = rhwp::serializer::serialize_hwpx(&source).expect("synthetic HWPX");
    let original = DocumentCore::from_bytes(&bytes).expect("public HWPX import");
    let (original_boxes, original_following) = geometry(&original, &source);
    let host = &original.document().sections[0].paragraphs[1];
    assert_eq!(host.text, "  ", "host whitespace is source content");
    assert!(
        host.serializable_line_segs().is_empty(),
        "generated rows remain layout only"
    );
    assert_eq!(
        host.line_segs.len(),
        2,
        "full-width tables wrap into individual rows"
    );

    for (line, height) in host.line_segs.iter().zip(HEIGHTS) {
        assert_eq!(line.line_height, height as i32 + 2 * i32::from(MARGIN));
    }
    assert_eq!(
        host.line_seg_text_start(1),
        10,
        "terminal table owns its raw UTF-16 anchor"
    );

    let output = original
        .export_hwp_with_adapter()
        .expect("public HWP export");
    let reopened = DocumentCore::from_bytes(&output).expect("public HWP reopen");
    let (output_boxes, output_following) = geometry(&reopened, &source);
    for (before, after) in original_boxes.iter().zip(output_boxes) {
        assert!(
            (before.0 - after.0).abs() < 0.5
                && (before.1 - after.1).abs() < 0.5
                && (before.2 - after.2).abs() < 0.5
                && (before.3 - after.3).abs() < 0.5,
            "roundtrip preserves actual table geometry: {before:?} versus {after:?}"
        );
    }

    assert!((original_following.0 - output_following.0).abs() < 0.5
        && (original_following.1 - output_following.1).abs() < 0.5,
        "roundtrip preserves following text geometry: {original_following:?} versus {output_following:?}");
}
