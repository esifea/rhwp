//! Synthetic contracts for cache-free TAC host rows and their final consumers.
//! Expected bounds use declared cell sizes and the input page body.
#![cfg(not(target_arch = "wasm32"))]

use rhwp::document_core::DocumentCore;
use rhwp::model::control::Control;
use rhwp::model::document::{Document, Section, SectionDef};
use rhwp::model::page::PageDef;
use rhwp::model::paragraph::{CharShapeRef, LineSeg, Paragraph};
use rhwp::model::table::{Cell, Table, TablePageBreak};
use rhwp::renderer::render_tree::{RenderNode, RenderNodeType};

const BODY_WIDTH: u32 = 45000;
const MARGIN: i16 = 150;
const BEFORE: &str = "BeforeMarker";
const AFTER: &str = "FollowingMarker";
const LABELS: [&str; 2] = ["FirstCell", "SecondCell"];
const EPS: f64 = 0.5;

type Bounds = (u32, f64, f64, f64, f64);

fn px(units: u32) -> f64 {
    f64::from(units) / 75.0
}

fn paragraph(value: &str, cached: bool) -> Paragraph {
    let mut raw = 0;
    let offsets = value
        .chars()
        .map(|ch| {
            let offset = raw;
            raw += ch.len_utf16() as u32;
            offset
        })
        .collect();
    Paragraph {
        text: value.into(),
        char_count: raw + 1,
        char_offsets: offsets,
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
                segment_width: BODY_WIDTH as i32,
                ..Default::default()
            }]
        } else {
            Vec::new()
        },
        ..Default::default()
    }
}

fn table(index: usize, width: u32, height: u32) -> Table {
    let mut value = Table {
        row_count: 1,
        col_count: 1,
        page_break: TablePageBreak::None,
        outer_margin_top: MARGIN,
        outer_margin_bottom: MARGIN,
        ..Default::default()
    };
    value.common.width = width;
    value.common.height = height;
    value.common.treat_as_char = true;
    value.common.flow_with_text = true;
    value.common.text_wrap = rhwp::model::shape::TextWrap::TopAndBottom;
    value.common.vert_rel_to = rhwp::model::shape::VertRelTo::Para;
    value.common.margin.top = MARGIN;
    value.common.margin.bottom = MARGIN;
    value.cells.push(Cell {
        row: 0,
        col: 0,
        width,
        height,
        row_span: 1,
        col_span: 1,
        paragraphs: vec![paragraph(LABELS[index], true)],
        ..Default::default()
    });
    value.rebuild_grid();
    value
}

fn fixture(widths: [u32; 2], heights: [u32; 2], parts: [&str; 3], body: u32) -> Document {
    let mut host = paragraph(&parts.concat(), false);
    host.char_offsets.clear();
    let mut raw = 0;
    for (index, part) in parts.into_iter().enumerate() {
        if index > 0 {
            raw += 8;
        }
        for ch in part.chars() {
            host.char_offsets.push(raw);
            raw += ch.len_utf16() as u32;
        }
    }
    host.char_count = raw + 1;
    host.controls = (0..2)
        .map(|i| Control::Table(Box::new(table(i, widths[i], heights[i]))))
        .collect();
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
    page.width = BODY_WIDTH + 15000;
    page.height = body + 15000;
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
        paragraphs: vec![paragraph(BEFORE, true), host, paragraph(AFTER, true)],
        ..Default::default()
    });
    document
}

fn open(source: &Document) -> DocumentCore {
    let bytes = rhwp::serializer::serialize_hwpx(source).expect("synthetic HWPX serialization");
    DocumentCore::from_bytes(&bytes).expect("public HWPX import")
}

fn reopen(core: &DocumentCore) -> DocumentCore {
    let bytes = core.export_hwp_with_adapter().expect("public HWP export");
    DocumentCore::from_bytes(&bytes).expect("public HWP reopen")
}

fn inside(inner: Bounds, outer: Bounds) -> bool {
    inner.0 == outer.0
        && inner.1 >= outer.1 - EPS
        && inner.2 >= outer.2 - EPS
        && inner.1 + inner.3 <= outer.1 + outer.3 + EPS
        && inner.2 + inner.4 <= outer.2 + outer.4 + EPS
}

fn rendered_text(node: &RenderNode) -> String {
    let mut result = match &node.node_type {
        RenderNodeType::TextRun(run) => run.text.clone(),
        _ => String::new(),
    };
    for child in &node.children {
        result.push_str(&rendered_text(child));
    }
    result
}

fn collect(
    node: &RenderNode,
    page: u32,
    tables: &mut Vec<(String, Bounds)>,
    runs: &mut Vec<(String, Bounds)>,
) {
    let b = &node.bbox;
    let bounds = (page, b.x, b.y, b.width, b.height);
    match &node.node_type {
        RenderNodeType::Table(_) => tables.push((rendered_text(node), bounds)),
        RenderNodeType::TextRun(run) => runs.push((run.text.clone(), bounds)),
        _ => {}
    }
    for child in &node.children {
        collect(child, page, tables, runs);
    }
}

fn geometry(core: &DocumentCore, source: &Document, pages: u32) -> [Bounds; 2] {
    assert_eq!(
        core.page_count(),
        pages,
        "page count follows independently declared body budget"
    );
    source_properties(core, source);
    let page = &source.sections[0].section_def.page_def;
    let mut tables = Vec::new();
    let mut runs = Vec::new();
    for index in 0..core.page_count() {
        let tree = core
            .build_page_render_tree(index)
            .expect("actual final render tree");
        collect(&tree.root, index, &mut tables, &mut runs);
    }
    assert_eq!(tables.len(), 2, "each unsplit table appears exactly once");
    let body = (
        0,
        px(page.margin_left),
        px(page.margin_top),
        px(BODY_WIDTH),
        px(page.height - page.margin_top - page.margin_bottom),
    );
    let boxes = std::array::from_fn(|index| {
        let matching = tables
            .iter()
            .filter(|(text, _)| text.contains(LABELS[index]))
            .collect::<Vec<_>>();
        assert_eq!(matching.len(), 1, "table retains its own cell label");
        let bounds = matching[0].1;
        let owner = match &source.sections[0].paragraphs[1].controls[index] {
            Control::Table(table) => table,
            _ => panic!("synthetic table owner"),
        };
        assert!(
            (bounds.3 - px(owner.common.width)).abs() < EPS,
            "declared table width: {bounds:?}"
        );
        assert!(
            (bounds.4 - px(owner.common.height)).abs() < EPS,
            "declared cell height: {bounds:?}"
        );
        assert!(
            inside(bounds, (bounds.0, body.1, body.2, body.3, body.4)),
            "table fits page body: {bounds:?}"
        );
        let cell_runs = runs
            .iter()
            .filter(|(text, _)| text.contains(LABELS[index]))
            .collect::<Vec<_>>();
        assert_eq!(cell_runs.len(), 1, "cell text appears exactly once");
        assert!(
            inside(cell_runs[0].1, bounds),
            "cell text stays within its own table"
        );
        bounds
    });
    let preceding = runs
        .iter()
        .filter(|(text, _)| text.contains(BEFORE))
        .collect::<Vec<_>>();
    assert_eq!(preceding.len(), 1, "preceding text appears exactly once");
    assert!(
        inside(preceding[0].1, body),
        "preceding text fits first page body"
    );
    assert!(
        boxes[0].0 > preceding[0].1 .0 || boxes[0].2 >= preceding[0].1 .2 + preceding[0].1 .4 - EPS,
        "first table clears preceding paragraph"
    );
    let following = runs
        .iter()
        .filter(|(text, _)| text.contains(AFTER))
        .collect::<Vec<_>>();
    assert_eq!(following.len(), 1, "following text appears exactly once");
    let tail = following[0].1;
    assert!(
        inside(tail, (tail.0, body.1, body.2, body.3, body.4)),
        "following text fits body"
    );
    for table in boxes {
        assert!(
            tail.0 > table.0 || (tail.0 == table.0 && tail.2 >= table.2 + table.4 - EPS),
            "following paragraph clears every table: tail={tail:?}, table={table:?}"
        );
    }
    let actual = runs
        .iter()
        .map(|(text, _)| text.as_str())
        .collect::<String>();
    let expected = format!(
        "{BEFORE}{}{AFTER}{}{}",
        source.sections[0].paragraphs[1].text, LABELS[0], LABELS[1]
    );
    let units = |value: &str| {
        let mut result = value
            .chars()
            .filter(|ch| !ch.is_whitespace())
            .collect::<Vec<_>>();
        result.sort_unstable();
        result
    };
    assert_eq!(
        units(&actual),
        units(&expected),
        "all authored text survives exactly once"
    );
    boxes
}

fn ordered(boxes: [Bounds; 2]) {
    assert!(
        boxes[1].0 > boxes[0].0
            || (boxes[1].0 == boxes[0].0 && boxes[1].2 >= boxes[0].2 + boxes[0].4 - EPS),
        "ordered full-width tables do not overlap: {boxes:?}"
    );
}

#[test]
fn tall_first_short_second_keep_their_own_bands_after_roundtrip() {
    let source = fixture([BODY_WIDTH; 2], [37400, 7200], ["", "  ", ""], 60000);
    let original = open(&source);
    ordered(geometry(&original, &source, 1));
    let reopened = reopen(&original);
    ordered(geometry(&reopened, &source, 1));
}

#[test]
fn narrow_tables_share_one_physical_line_and_baseline_after_roundtrip() {
    let source = fixture([15000; 2], [7200, 4500], ["", "  ", ""], 60000);
    let original = open(&source);
    for core in [&original, &reopen(&original)] {
        let boxes = geometry(core, &source, 1);
        assert_eq!(boxes[0].0, boxes[1].0, "same page");
        assert!(
            boxes[1].1 >= boxes[0].1 + boxes[0].3 - EPS,
            "side-by-side tables do not overlap: {boxes:?}"
        );
        let baseline = |b: Bounds| b.2 + 0.85 * b.4;
        assert!(
            (baseline(boxes[0]) - baseline(boxes[1])).abs() < EPS,
            "shared baseline uses the established inline-table 0.85 height contract: {boxes:?}"
        );
        assert!(
            (boxes[0].2 - boxes[1].2).abs() < px(7200),
            "tables occupy the same physical band"
        );
    }
    assert_eq!(
        original.document().sections[0].paragraphs[1]
            .line_segs
            .len(),
        1,
        "combined 30000 HU widths plus two spaces fit 45000 HU"
    );
}

#[test]
fn narrow_shared_row_fits_a_body_that_cannot_hold_two_row_charges() {
    let source = fixture([15000; 2], [7200, 4500], ["", "  ", ""], 12000);
    let original = open(&source);
    for core in [&original, &reopen(&original)] {
        let boxes = geometry(core, &source, 1);
        assert!(
            (boxes[0].2 + 0.85 * boxes[0].4 - boxes[1].2 - 0.85 * boxes[1].4).abs() < EPS,
            "one shared baseline and one row budget: {boxes:?}"
        );
    }
}

#[test]
fn first_section_host_preserves_structural_control_raw_anchors() {
    let mut source = fixture([BODY_WIDTH; 2], [7200, 11400], ["", "  ", ""], 60000);
    source.sections[0].paragraphs.remove(0);
    let section_def = source.sections[0].section_def.clone();
    let host = &mut source.sections[0].paragraphs[0];
    host.controls
        .insert(0, Control::ColumnDef(Default::default()));
    host.controls
        .insert(0, Control::SectionDef(Box::new(section_def)));
    for offset in &mut host.char_offsets {
        *offset += 16;
    }
    host.char_count += 16;
    let original = open(&source);
    for core in [&original, &reopen(&original)] {
        assert_eq!(core.page_count(), 1);
        let host = &core.document().sections[0].paragraphs[0];
        let table_indices = host
            .controls
            .iter()
            .enumerate()
            .filter_map(|(ci, control)| matches!(control, Control::Table(_)).then_some(ci))
            .collect::<Vec<_>>();
        assert_eq!(table_indices.len(), 2);
        assert!(
            table_indices[0] >= 2,
            "structural controls precede the tables"
        );
        assert_eq!(
            host.char_offsets,
            vec![24, 25],
            "text offsets include both structural slots"
        );
        assert_eq!(
            host.line_seg_text_start(1),
            26,
            "second row retains the raw owner anchor"
        );
        let tree = core
            .build_page_render_tree(0)
            .expect("first-section final paint");
        let mut tables = Vec::new();
        let mut runs = Vec::new();
        collect(&tree.root, 0, &mut tables, &mut runs);
        assert_eq!(tables.len(), 2);
        let boxes = std::array::from_fn(|index| {
            let matching = tables
                .iter()
                .filter(|(text, _)| text.contains(LABELS[index]))
                .collect::<Vec<_>>();
            assert_eq!(matching.len(), 1, "each raw owner paints exactly once");
            let bounds = matching[0].1;
            assert!((bounds.3 - px(BODY_WIDTH)).abs() < EPS);
            assert!((bounds.4 - px([7200, 11400][index])).abs() < EPS);
            bounds
        });
        ordered(boxes);
        let following = runs
            .iter()
            .filter(|(text, _)| text.contains(AFTER))
            .collect::<Vec<_>>();
        assert_eq!(following.len(), 1);
        assert!(following[0].1 .2 >= boxes[1].2 + boxes[1].4 - EPS);
    }
}

#[test]
fn missing_rows_keep_following_authored_cache_positions_in_export_projection() {
    let mut source = fixture([BODY_WIDTH; 2], [7200, 11400], ["", "  ", ""], 60000);
    source.sections[0].paragraphs[0].line_segs.clear();
    source.sections[0].paragraphs[2].line_segs[0].vertical_pos = 22300;
    let mut later = paragraph("LaterMarker", true);
    later.line_segs[0].vertical_pos = 23900;
    source.sections[0].paragraphs.push(later);
    let original = open(&source);
    for core in [&original, &reopen(&original)] {
        let projected = core.source_line_cache_snapshot();
        for index in [2, 3] {
            assert_eq!(
                projected.sections[0].paragraphs[index].line_segs[0].vertical_pos,
                source.sections[0].paragraphs[index].line_segs[0].vertical_pos,
                "a layout rebase preserves the following paragraph's authored cache"
            );
        }
    }
}

#[test]
fn insufficient_body_keeps_two_pages_and_all_following_content() {
    let source = fixture([BODY_WIDTH; 2], [37400, 7200], ["", "  ", ""], 42000);
    let original = open(&source);
    let boxes = geometry(&original, &source, 2);
    ordered(boxes);
    assert!(
        boxes[1].0 > boxes[0].0,
        "44600 HU of table frames cannot fit a 42000 HU body"
    );
    let reopened = reopen(&original);
    let boxes = geometry(&reopened, &source, 2);
    ordered(boxes);
    assert!(boxes[1].0 > boxes[0].0, "roundtrip retains real overflow");
}

fn source_properties(core: &DocumentCore, source: &Document) {
    let actual = core.document();
    let before_char = &source.doc_info.char_shapes[0];
    let after_char = &actual.doc_info.char_shapes[0];
    assert_eq!(
        (after_char.base_size, after_char.bold, after_char.italic),
        (before_char.base_size, before_char.bold, before_char.italic),
        "character style properties survive"
    );
    let before_para = &source.doc_info.para_shapes[0];
    let after_para = &actual.doc_info.para_shapes[0];
    assert_eq!(
        (
            after_para.line_spacing,
            after_para.spacing_before,
            after_para.spacing_after
        ),
        (
            before_para.line_spacing,
            before_para.spacing_before,
            before_para.spacing_after
        ),
        "paragraph spacing properties survive"
    );
    let before_host = &source.sections[0].paragraphs[1];
    let after_host = &actual.sections[0].paragraphs[1];
    assert_eq!(
        after_host.text, before_host.text,
        "visible host text is source content"
    );
    for (before, after) in before_host.controls.iter().zip(&after_host.controls) {
        if let (Control::Table(before), Control::Table(after)) = (before, after) {
            assert_eq!(
                (after.common.width, after.common.height, after.page_break),
                (before.common.width, before.common.height, before.page_break),
                "table source dimensions and split policy survive"
            );
            assert_eq!(
                (after.outer_margin_top, after.outer_margin_bottom),
                (before.outer_margin_top, before.outer_margin_bottom),
                "table margins survive"
            );
            assert_eq!(after.cells.len(), before.cells.len());
            for (a, b) in after.cells.iter().zip(&before.cells) {
                assert_eq!(
                    (a.row, a.col, a.row_span, a.col_span, a.width, a.height),
                    (b.row, b.col, b.row_span, b.col_span, b.width, b.height),
                    "cell source geometry survives"
                );
                assert_eq!(a.paragraphs[0].text, b.paragraphs[0].text);
            }
        }
    }
}

fn host_text_geometry(core: &DocumentCore, source: &Document, markers: [&str; 3]) {
    let boxes = geometry(core, source, 1);
    ordered(boxes);
    let tree = core
        .build_page_render_tree(0)
        .expect("actual host text geometry");
    let mut tables = Vec::new();
    let mut runs = Vec::new();
    collect(&tree.root, 0, &mut tables, &mut runs);
    let text_boxes: [Bounds; 3] = std::array::from_fn(|index| {
        let matches = runs
            .iter()
            .filter(|(text, _)| text.contains(markers[index]))
            .collect::<Vec<_>>();
        assert_eq!(matches.len(), 1, "each host marker is painted exactly once");
        matches[0].1
    });
    assert!(
        text_boxes[0].2 + text_boxes[0].4 <= boxes[0].2 + EPS,
        "prefix clears first full-width table: {text_boxes:?}, {boxes:?}"
    );
    assert!(
        text_boxes[1].2 >= boxes[0].2 + boxes[0].4 - EPS,
        "between text clears first table"
    );
    assert!(
        text_boxes[1].2 + text_boxes[1].4 <= boxes[1].2 + EPS,
        "between text clears second table"
    );
    assert!(
        text_boxes[2].2 >= boxes[1].2 + boxes[1].4 - EPS,
        "suffix clears second table"
    );
    let following = runs
        .iter()
        .find(|(text, _)| text.contains(AFTER))
        .expect("following marker")
        .1;
    assert!(
        following.2 >= text_boxes[2].2 + text_boxes[2].4 - EPS,
        "following paragraph clears suffix"
    );
}

#[test]
fn visible_prefix_between_and_suffix_keep_their_own_rows_after_roundtrip() {
    let source = fixture(
        [BODY_WIDTH; 2],
        [7200, 11400],
        ["PrefixMarker", "BetweenMarker", "SuffixMarker"],
        60000,
    );
    let original = open(&source);
    host_text_geometry(
        &original,
        &source,
        ["PrefixMarker", "BetweenMarker", "SuffixMarker"],
    );
    host_text_geometry(
        &reopen(&original),
        &source,
        ["PrefixMarker", "BetweenMarker", "SuffixMarker"],
    );
}

#[test]
fn explicit_breaks_preserve_text_rows_and_table_order_after_roundtrip() {
    let source = fixture(
        [BODY_WIDTH; 2],
        [7200, 11400],
        ["PrefixMarker\n", "\nBetweenMarker\n", "\nSuffixMarker"],
        60000,
    );
    let original = open(&source);
    host_text_geometry(
        &original,
        &source,
        ["PrefixMarker", "BetweenMarker", "SuffixMarker"],
    );
    host_text_geometry(
        &reopen(&original),
        &source,
        ["PrefixMarker", "BetweenMarker", "SuffixMarker"],
    );
}

#[test]
fn non_bmp_between_tables_preserves_terminal_utf16_owner_after_roundtrip() {
    let source = fixture([BODY_WIDTH; 2], [7200, 11400], ["", "😀", ""], 60000);
    assert_eq!(source.sections[0].paragraphs[1].char_offsets, vec![8]);
    assert_eq!(
        source.sections[0].paragraphs[1].char_count, 19,
        "two 8-unit controls, two UTF-16 units and terminator"
    );
    let original = open(&source);
    let host = &original.document().sections[0].paragraphs[1];
    assert_eq!(
        host.line_seg_text_start(host.line_segs.len() - 1),
        10,
        "terminal table starts after both surrogate units"
    );
    for core in [&original, &reopen(&original)] {
        between_marker_geometry(core, &source, "😀");
    }
}

fn cached_fixture() -> Document {
    let mut source = fixture([BODY_WIDTH; 2], [7200, 11400], ["", "  ", ""], 60000);
    source.doc_info.char_shapes[0].bold = true;
    source.doc_info.char_shapes[0].italic = true;
    source.doc_info.para_shapes[0].spacing_before = 120;
    source.doc_info.para_shapes[0].spacing_after = 180;
    let host = &mut source.sections[0].paragraphs[1];
    host.line_segs = [7200, 11400]
        .into_iter()
        .enumerate()
        .map(|(index, height)| LineSeg {
            text_start: if index == 0 { 0 } else { 10 },
            vertical_pos: if index == 0 { 1900 } else { 10000 },
            line_height: height + 300,
            text_height: height + 300,
            baseline_distance: ((height + 300) as f64 * 0.85).round() as i32,
            line_spacing: 600,
            segment_width: BODY_WIDTH as i32,
            tag: LineSeg::TAG_SINGLE_SEGMENT_LINE,
            ..Default::default()
        })
        .collect();
    // Authored positions continue the 7500+600 and 11700+600 HU row bands.
    source.sections[0].paragraphs[2].line_segs[0].vertical_pos = 22300;
    source
}

#[test]
fn valid_synthetic_cached_rows_and_styles_remain_source_backed() {
    let source = cached_fixture();
    let original = open(&source);
    for core in [&original, &reopen(&original)] {
        ordered(geometry(core, &source, 1));
        let host = &core.document().sections[0].paragraphs[1];
        let expected = &source.sections[0].paragraphs[1];
        assert_eq!(
            host.serializable_line_segs().len(),
            2,
            "authored cached rows remain serializable"
        );
        for (index, (actual, expected)) in host
            .serializable_line_segs()
            .iter()
            .zip(&expected.line_segs)
            .enumerate()
        {
            assert_eq!(
                host.line_seg_text_start(index),
                expected.text_start,
                "source row anchor survives"
            );
            let source_vpos = host
                .source_line_seg_vertical_pos
                .as_ref()
                .and_then(|positions| positions.get(index))
                .copied()
                .unwrap_or(actual.vertical_pos);
            assert_eq!(
                source_vpos, expected.vertical_pos,
                "authored source row vertical position survives"
            );
            assert_eq!(actual.tag, expected.tag, "authored row flags survive");
            assert_eq!(
                (
                    actual.line_height,
                    actual.text_height,
                    actual.baseline_distance,
                    actual.line_spacing,
                    actual.segment_width,
                    actual.column_start
                ),
                (
                    expected.line_height,
                    expected.text_height,
                    expected.baseline_distance,
                    expected.line_spacing,
                    expected.segment_width,
                    expected.column_start
                ),
                "source row metrics are preserved"
            );
        }
    }
}

#[test]
fn edited_source_backed_host_reflows_visible_between_text_and_terminal_table() {
    let source = cached_fixture();
    let mut original = open(&source);
    original
        .insert_text_native(0, 1, 1, "EditedMarker")
        .expect("public edit inside source-backed host");
    let expected = original.document().clone();
    assert_eq!(
        expected.sections[0].paragraphs[1].text, " EditedMarker ",
        "edit preserves both authored spaces"
    );
    assert_eq!(
        expected.sections[0].paragraphs[1].line_segs_are_layout_only,
        Some(false),
        "regenerated host retains source-backed provenance"
    );
    for core in [&original, &reopen(&original)] {
        between_marker_geometry(core, &expected, "EditedMarker");
    }
}

fn between_marker_geometry(core: &DocumentCore, source: &Document, marker: &str) {
    let boxes = geometry(core, source, 1);
    ordered(boxes);
    let tree = core
        .build_page_render_tree(0)
        .expect("actual between-marker geometry");
    let mut tables = Vec::new();
    let mut runs = Vec::new();
    collect(&tree.root, 0, &mut tables, &mut runs);
    let matches = runs
        .iter()
        .filter(|(text, _)| text.contains(marker))
        .collect::<Vec<_>>();
    assert_eq!(matches.len(), 1, "between marker is painted once");
    assert!(
        matches[0].1 .2 >= boxes[0].2 + boxes[0].4 - EPS
            && matches[0].1 .2 + matches[0].1 .4 <= boxes[1].2 + EPS,
        "between marker clears both full-width tables: marker={:?}, tables={boxes:?}",
        matches[0].1
    );
}

#[test]
fn generated_whitespace_host_grown_rowbreak_table_keeps_block_fragments() {
    let mut source = fixture([BODY_WIDTH; 2], [7200, 11400], ["", "", ""], 27000);
    let mut grown = table(0, BODY_WIDTH, 18 * 1300);
    grown.row_count = 18;
    grown.col_count = 2;
    grown.page_break = TablePageBreak::RowBreak;
    grown.padding.top = 150;
    grown.padding.bottom = 150;
    grown.cells.clear();
    for row in 0..18 {
        for col in 0..2 {
            let value = if col == 0 {
                format!("GrowthLabel{row}")
            } else if row == 1 || row == 2 {
                "Long wrapped synthetic content Long wrapped synthetic content Long wrapped synthetic content".into()
            } else {
                format!("GrowthValue{row}")
            };
            grown.cells.push(Cell {
                row,
                col,
                width: BODY_WIDTH / 2,
                height: 1300,
                row_span: 1,
                col_span: 1,
                paragraphs: vec![paragraph(&value, false)],
                ..Default::default()
            });
        }
    }
    grown.rebuild_grid();
    let mut host = paragraph("  ", false);
    for offset in &mut host.char_offsets {
        *offset += 8;
    }
    host.char_count += 8;
    host.controls = vec![Control::Table(Box::new(grown))];
    source.sections[0].paragraphs[1] = host;
    let original = open(&source);
    for core in [&original, &reopen(&original)] {
        assert!(
            core.page_count() >= 2,
            "fresh row growth exceeds the 27000 HU body"
        );
        let body_top = px(7500);
        let body_bottom = px(7500 + 27000);
        let mut tables = Vec::new();
        let mut runs = Vec::new();
        for page in 0..core.page_count() {
            let tree = core
                .build_page_render_tree(page)
                .expect("grown host final paint");
            collect(&tree.root, page, &mut tables, &mut runs);
        }
        assert!(
            tables.len() >= 2,
            "splittable grown table needs physical fragments"
        );
        for (_, bounds) in &tables {
            assert!(
                bounds.2 >= body_top - EPS && bounds.2 + bounds.4 <= body_bottom + EPS,
                "grown fragment fits its physical body: {bounds:?}"
            );
        }
        for (_, bounds) in &runs {
            assert!(
                bounds.2 >= body_top - EPS && bounds.2 + bounds.4 <= body_bottom + EPS,
                "authored text fits its physical body: {bounds:?}"
            );
        }
        let actual = runs
            .iter()
            .map(|(text, _)| text.as_str())
            .collect::<String>();
        let mut expected = format!("{BEFORE}  {AFTER}");
        let Control::Table(table) = &source.sections[0].paragraphs[1].controls[0] else {
            panic!("synthetic grown table");
        };
        for cell in &table.cells {
            expected.push_str(&cell.paragraphs[0].text);
        }
        let units = |text: &str| {
            let mut units = text
                .chars()
                .filter(|c| !c.is_whitespace())
                .collect::<Vec<_>>();
            units.sort_unstable();
            units
        };
        assert_eq!(
            units(&actual),
            units(&expected),
            "host and every cell survive once"
        );
        let following = runs
            .iter()
            .find(|(text, _)| text.contains(AFTER))
            .expect("following body survives")
            .1;
        for (_, bounds) in &tables {
            assert!(
                following.0 > bounds.0
                    || (following.0 == bounds.0 && following.2 >= bounds.2 + bounds.4 - EPS),
                "following text clears every grown fragment: {following:?}, {bounds:?}"
            );
        }
    }
}

#[test]
fn native_missing_cache_host_uses_the_same_generated_rows() {
    let source = fixture(
        [BODY_WIDTH; 2],
        [7200, 11400],
        ["", "NativeBetween", ""],
        60000,
    );
    let bytes = rhwp::serializer::serialize_document(&source).expect("synthetic native HWP");
    let mut original = DocumentCore::from_bytes(&bytes).expect("public native import");
    original.reflow_linesegs_on_demand();
    let mut reopened = reopen(&original);
    reopened.reflow_linesegs_on_demand();
    for core in [&original, &reopened] {
        between_marker_geometry(core, &source, "NativeBetween");
    }
}

#[test]
fn generated_rows_reserve_outer_margins_once_at_exact_body_boundary() {
    // 1600 + 7500 + 600 + 11700 + 600 + 1000 = 23000 HU.
    for body in [23000, 23200] {
        let source = fixture([BODY_WIDTH; 2], [7200, 11400], ["", "  ", ""], body);
        let original = open(&source);
        for core in [&original, &reopen(&original)] {
            ordered(geometry(core, &source, 1));
        }
    }
}

fn single_host(source: &mut Document, table: Table) {
    let mut host = paragraph("  ", false);
    for offset in &mut host.char_offsets {
        *offset += 8;
    }
    host.char_count += 8;
    host.controls = vec![Control::Table(Box::new(table))];
    source.sections[0].paragraphs[1] = host;
}

fn final_objects(core: &DocumentCore) -> (Vec<(String, Bounds)>, Vec<(String, Bounds)>) {
    let mut tables = Vec::new();
    let mut runs = Vec::new();
    for page in 0..core.page_count() {
        let tree = core
            .build_page_render_tree(page)
            .expect("final occupancy paint");
        collect(&tree.root, page, &mut tables, &mut runs);
    }
    (tables, runs)
}

fn visible_replay_bounds(core: &DocumentCore, text: &str, bounds: Bounds) -> Bounds {
    let Some(visible_end) = text
        .chars()
        .collect::<Vec<_>>()
        .iter()
        .rposition(|ch| !ch.is_whitespace())
    else {
        return bounds;
    };
    if visible_end + 1 == text.chars().count() {
        return bounds;
    }
    let layout: serde_json::Value = serde_json::from_str(
        &core
            .get_page_text_layout_native(bounds.0)
            .expect("final replay layout"),
    )
    .expect("replay JSON");
    let run = layout["runs"]
        .as_array()
        .expect("replay runs")
        .iter()
        .find(|run| {
            run["text"].as_str() == Some(text)
                && (run["x"].as_f64().expect("run x") - bounds.1).abs() < EPS
                && (run["y"].as_f64().expect("run y") - bounds.2).abs() < EPS
        })
        .expect("matching emitted run");
    let advance = run["charX"][visible_end + 1]
        .as_f64()
        .expect("last visible replay boundary");
    (bounds.0, bounds.1, bounds.2, advance, bounds.4)
}

fn visible_cell_runs(core: &DocumentCore, node: &RenderNode, page: u32, cell: Option<Bounds>) {
    let bbox = &node.bbox;
    let bounds = (page, bbox.x, bbox.y, bbox.width, bbox.height);
    let cell = if matches!(node.node_type, RenderNodeType::TableCell(_)) {
        Some(bounds)
    } else {
        cell
    };
    if let (Some(cell), RenderNodeType::TextRun(run)) = (cell, &node.node_type) {
        if run.text.chars().any(|ch| !ch.is_whitespace()) {
            let visible = visible_replay_bounds(core, &run.text, bounds);
            assert!(
                inside(visible, cell),
                "visible replay stays in its own cell: {:?}, {visible:?}, {cell:?}",
                run.text
            );
        }
    }
    for child in &node.children {
        visible_cell_runs(core, child, page, cell);
    }
}

fn atomic_grown_frame_geometry(width: u32, classify_nonink_spaces: bool) {
    let body = 42000;
    let mut source = fixture([BODY_WIDTH; 2], [7200, 11400], ["", "", ""], body);
    let mut grown = table(0, width, 18 * 1300);
    grown.row_count = 18;
    grown.col_count = 2;
    grown.padding.top = 150;
    grown.padding.bottom = 150;
    grown.cells.clear();
    for row in 0..18 {
        for col in 0..2 {
            let value = if col == 0 {
                format!("GrowthLabel{row}")
            } else if row == 1 || row == 2 {
                "Long wrapped synthetic content Long wrapped synthetic content Long wrapped synthetic content".into()
            } else {
                format!("GrowthValue{row}")
            };
            grown.cells.push(Cell {
                row,
                col,
                width: width / 2,
                height: 1300,
                row_span: 1,
                col_span: 1,
                paragraphs: vec![paragraph(&value, false)],
                ..Default::default()
            });
        }
    }
    grown.rebuild_grid();
    single_host(&mut source, grown);
    let before = &mut source.sections[0].paragraphs[0].line_segs[0];
    before.line_height = 15000;
    before.text_height = 15000;
    before.baseline_distance = 12750;
    let original = open(&source);
    for core in [&original, &reopen(&original)] {
        let (tables, runs) = final_objects(core);
        assert_eq!(tables.len(), 1, "None keeps one complete table frame");
        let bounds = tables[0].1;
        assert!(
            bounds.4 > px(27000),
            "actual frame grows beyond first-page remainder: {bounds:?}"
        );
        assert_eq!(bounds.0, 1, "grown atomic frame moves to the second body");
        let body_bounds = (bounds.0, px(7500), px(7500), px(BODY_WIDTH), px(body));
        assert!(
            inside(bounds, body_bounds),
            "whole grown frame fits body: {bounds:?}"
        );
        assert_eq!(
            core.document().sections[0].paragraphs[1].text,
            "  ",
            "source spaces survive reflow and export"
        );
        let mut host_text = String::new();
        for page in 0..core.page_count() {
            let tree = core
                .build_page_render_tree(page)
                .expect("host source replay");
            host_text.push_str(&body_host_text(&tree.root));
            visible_cell_runs(core, &tree.root, page, None);
        }
        assert_eq!(host_text, "  ", "both host spaces replay exactly once");
        for (text, natural_bounds) in &runs {
            let run = visible_replay_bounds(core, text, *natural_bounds);
            if classify_nonink_spaces && text.chars().all(|ch| ch == ' ') {
                // Natural whitespace advances are nonink metadata at a soft wrap.
                assert_eq!(run.0, bounds.0, "spaces stay on their table owner page");
                assert!(
                    run.2 >= body_bounds.2 - EPS
                        && run.2 + run.4 <= body_bounds.2 + body_bounds.4 + EPS,
                    "nonink whitespace keeps the occupied row band: {run:?}"
                );
            } else {
                assert!(
                    inside(
                        run,
                        (
                            run.0,
                            body_bounds.1,
                            body_bounds.2,
                            body_bounds.3,
                            body_bounds.4
                        )
                    ),
                    "text fits body: {text:?} {run:?}"
                );
            }
            if text.starts_with("Growth")
                || text.contains("synthetic")
                || text.contains("wrapped")
                || text.contains("Long")
                || text.contains("content")
            {
                assert!(
                    inside(run, bounds),
                    "grown cell text fits complete frame: {text:?} {run:?}"
                );
            }
        }
        let actual = runs
            .iter()
            .map(|(text, _)| text.as_str())
            .collect::<String>();
        let Control::Table(table) = &source.sections[0].paragraphs[1].controls[0] else {
            panic!("atomic owner")
        };
        let mut expected = format!("{BEFORE}{AFTER}");
        for cell in &table.cells {
            expected.push_str(&cell.paragraphs[0].text);
        }
        let units = |text: &str| {
            let mut value = text
                .chars()
                .filter(|ch| !ch.is_whitespace())
                .collect::<Vec<_>>();
            value.sort_unstable();
            value
        };
        assert_eq!(
            units(&actual),
            units(&expected),
            "every authored cell unit appears once"
        );
        let tail = runs
            .iter()
            .find(|(text, _)| text.contains(AFTER))
            .expect("following marker")
            .1;
        assert!(
            tail.0 > bounds.0 || (tail.0 == bounds.0 && tail.2 >= bounds.2 + bounds.4 - EPS),
            "following clears grown frame: {tail:?}, {bounds:?}"
        );
    }
}

fn body_host_text(node: &RenderNode) -> String {
    let mut text = match &node.node_type {
        RenderNodeType::TextRun(run) if run.para_index == Some(1) && run.cell_context.is_none() => {
            run.text.clone()
        }
        _ => String::new(),
    };
    for child in &node.children {
        text.push_str(&body_host_text(child));
    }
    text
}

#[test]
fn atomic_grown_frame_moves_whole_to_next_body_and_clears_following() {
    atomic_grown_frame_geometry(43800, false);
}

#[test]
fn full_width_atomic_growth_preserves_soft_wrap_spaces_and_visible_bounds() {
    atomic_grown_frame_geometry(BODY_WIDTH, true);
}

#[test]
fn atomic_vertical_captions_reserve_final_bounds_and_following_clearance() {
    use rhwp::model::shape::{Caption, CaptionDirection};
    for direction in [CaptionDirection::Bottom, CaptionDirection::Top] {
        let body = 11000;
        let mut source = fixture([BODY_WIDTH; 2], [7200, 11400], ["", "", ""], body);
        let mut object = table(0, 40000, 7200);
        object.caption = Some(Caption {
            direction,
            spacing: 600,
            paragraphs: vec![paragraph("CaptionMarker", true)],
            ..Default::default()
        });
        single_host(&mut source, object);
        // Uncaptioned flow plus following is 10700 HU; caption adds 1600 HU.
        let original = open(&source);
        for core in [&original, &reopen(&original)] {
            let (tables, runs) = final_objects(core);
            assert_eq!(tables.len(), 1);
            let bounds = tables[0].1;
            let caption = runs
                .iter()
                .filter(|(text, _)| text.contains("CaptionMarker"))
                .collect::<Vec<_>>();
            assert_eq!(caption.len(), 1, "caption paints once");
            let caption = caption[0].1;
            let tail = runs
                .iter()
                .find(|(text, _)| text.contains(AFTER))
                .expect("following marker")
                .1;
            assert!(
                inside(
                    bounds,
                    (bounds.0, px(7500), px(7500), px(BODY_WIDTH), px(body))
                ),
                "caption table stays inside body: {bounds:?}"
            );
            assert!(
                inside(
                    caption,
                    (caption.0, px(7500), px(7500), px(BODY_WIDTH), px(body))
                ),
                "caption stays inside body: {caption:?}"
            );
            assert_eq!(caption.0, bounds.0);
            match direction {
                CaptionDirection::Bottom => assert!(
                    caption.2 >= bounds.2 + bounds.4 + px(600) - EPS,
                    "bottom caption follows frame and spacing"
                ),
                CaptionDirection::Top => assert!(
                    bounds.2 >= caption.2 + caption.4 + px(600) - EPS,
                    "top caption precedes frame and spacing"
                ),
                _ => unreachable!(),
            }
            let occupied_bottom = (bounds.2 + bounds.4).max(caption.2 + caption.4);
            assert!(
                tail.0 > bounds.0 || (tail.0 == bounds.0 && tail.2 >= occupied_bottom - EPS),
                "following clears caption and frame: {tail:?}, {bounds:?}, {caption:?}"
            );
            assert_eq!(
                core.page_count(),
                2,
                "following marker exceeds exact occupied first body"
            );
        }
    }
}

#[test]
fn leading_spaces_wrap_before_full_width_atomic_table() {
    let source = fixture([BODY_WIDTH; 2], [7200, 11400], [" ", " ", ""], 60000);
    let original = open(&source);
    for core in [&original, &reopen(&original)] {
        ordered(geometry(core, &source, 1));
    }
}

#[test]
fn atomic_side_caption_width_wraps_the_next_table_and_stays_inside_body() {
    use rhwp::model::shape::{Caption, CaptionDirection};
    for direction in [CaptionDirection::Left, CaptionDirection::Right] {
        let source_width = 22000;
        let mut source = fixture([source_width; 2], [7200, 4500], ["", "  ", ""], 60000);
        let Control::Table(table) = &mut source.sections[0].paragraphs[1].controls[0] else {
            panic!("caption owner")
        };
        table.caption = Some(Caption {
            direction,
            width: 10000,
            spacing: 600,
            paragraphs: vec![paragraph("SideCaptionMarker", true)],
            ..Default::default()
        });
        // 22000 + 10000 + 600 + 22000 exceeds the 45000 HU row.
        let original = open(&source);
        for core in [&original, &reopen(&original)] {
            let (tables, runs) = final_objects(core);
            assert_eq!(tables.len(), 2);
            let first = tables
                .iter()
                .find(|(text, _)| text.contains(LABELS[0]))
                .expect("first frame")
                .1;
            let second = tables
                .iter()
                .find(|(text, _)| text.contains(LABELS[1]))
                .expect("second frame")
                .1;
            ordered([first, second]);
            let captions = runs
                .iter()
                .filter(|(text, _)| text.contains("SideCaptionMarker"))
                .collect::<Vec<_>>();
            assert_eq!(captions.len(), 1);
            let caption = captions[0].1;
            assert!(
                inside(caption, (0, px(7500), px(7500), px(BODY_WIDTH), px(60000))),
                "side caption fits body: {caption:?}"
            );
            match direction {
                CaptionDirection::Left => assert!(
                    caption.1 + caption.3 + px(600) <= first.1 + EPS,
                    "left caption clears table"
                ),
                CaptionDirection::Right => assert!(
                    caption.1 >= first.1 + first.3 + px(600) - EPS,
                    "right caption clears table"
                ),
                _ => unreachable!(),
            }
            let tail = runs
                .iter()
                .find(|(text, _)| text.contains(AFTER))
                .expect("following marker")
                .1;
            assert!(
                tail.2 >= second.2 + second.4 - EPS,
                "following clears the wrapped second table"
            );
        }
    }
}

#[test]
fn cell_derived_atomic_width_includes_column_spacing_in_final_frame() {
    let mut source = fixture([20000, 25000], [7200, 4500], ["", "  ", ""], 60000);
    let Control::Table(table) = &mut source.sections[0].paragraphs[1].controls[0] else {
        panic!("derived width owner")
    };
    table.common.width = 0;
    table.col_count = 2;
    table.cell_spacing = 300;
    table.cells[0].width = 10000;
    let mut second = table.cells[0].clone();
    second.col = 1;
    second.paragraphs = vec![paragraph("DerivedWidthMarker", true)];
    table.cells.push(second);
    table.rebuild_grid();
    let original = open(&source);
    for core in [&original, &reopen(&original)] {
        let (tables, runs) = final_objects(core);
        assert_eq!(tables.len(), 2);
        let first = tables
            .iter()
            .find(|(text, _)| text.contains(LABELS[0]))
            .expect("derived frame")
            .1;
        let second = tables
            .iter()
            .find(|(text, _)| text.contains(LABELS[1]))
            .expect("second frame")
            .1;
        assert!(
            (first.3 - px(20300)).abs() < EPS,
            "two10000 columns plus300 spacing: {first:?}"
        );
        ordered([first, second]);
        assert!(
            inside(first, (0, px(7500), px(7500), px(BODY_WIDTH), px(60000))),
            "derived frame stays in body"
        );
        assert_eq!(
            runs.iter()
                .filter(|(text, _)| text.contains("DerivedWidthMarker"))
                .count(),
            1
        );
        let tail = runs
            .iter()
            .find(|(text, _)| text.contains(AFTER))
            .expect("following marker")
            .1;
        assert!(
            tail.2 >= second.2 + second.4 - EPS,
            "following clears derived-width row flow"
        );
    }
}
