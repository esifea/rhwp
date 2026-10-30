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
                tag: LineSeg::TAG_SINGLE_SEGMENT_LINE,
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
    // Keep this block continuation independent of the helper's TAC ownership.
    grown.common.treat_as_char = false;
    grown.common.horz_rel_to = rhwp::model::shape::HorzRelTo::Column;
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
    source
        .doc_info
        .para_shapes
        .push(rhwp::model::style::ParaShape {
            line_spacing_type: rhwp::model::style::LineSpacingType::Fixed,
            line_spacing: 30000,
            ..Default::default()
        });
    let before = &mut source.sections[0].paragraphs[0];
    before.para_shape_id = 1;
    before.line_segs[0].line_spacing = 14000;
    let original = open(&source);
    let before = &original.document().sections[0].paragraphs[0];
    assert_eq!(
        original.document().doc_info.para_shapes[before.para_shape_id as usize].line_spacing,
        30000,
        "고정 줄간격은 두 배 스케일의 공통 IR 값을 보존한다"
    );
    assert_eq!(
        before.line_segs[0].line_height + before.line_segs[0].line_spacing,
        15000,
        "원본 캐시 검증 뒤에도 선행 본문은 200px를 소비한다"
    );
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
            (first.3 - px(20900)).abs() < EPS,
            "10000HU 두 열과 내부 1개·외곽 2개의 300HU 간격: {first:?}"
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

fn spaced_border_fixture(border_fill_id: u16) -> Document {
    let mut source = fixture([20000, 25000], [7200, 4500], ["", "  ", ""], 60000);
    let Control::Table(table) = &mut source.sections[0].paragraphs[1].controls[0] else {
        panic!("간격이 있는 표의 소유 컨트롤")
    };
    table.col_count = 2;
    table.cell_spacing = 300;
    table.cells[0].width = 10000;
    table.cells[0].border_fill_id = border_fill_id;
    let mut second = table.cells[0].clone();
    second.col = 1;
    second.paragraphs = vec![paragraph("SpacedRightMarker", true)];
    table.cells.push(second);
    table.rebuild_grid();
    source
}

fn border_nodes(node: &RenderNode) -> Vec<&RenderNode> {
    let mut nodes = Vec::new();
    if matches!(node.node_type, RenderNodeType::Line(_)) {
        nodes.push(node);
    }
    for child in &node.children {
        nodes.extend(border_nodes(child));
    }
    nodes
}

fn spaced_border_table(page: &RenderNode, border_fill_id: u16) -> &RenderNode {
    fn find(node: &RenderNode) -> Option<&RenderNode> {
        if matches!(node.node_type, RenderNodeType::Table(_))
            && rendered_text(node).contains("SpacedRightMarker")
        {
            return Some(node);
        }
        node.children.iter().find_map(find)
    }
    let table = find(page).expect("간격이 있는 표가 자기 표식을 유지한다");
    let cells = table
        .children
        .iter()
        .filter(|node| matches!(node.node_type, RenderNodeType::TableCell(_)))
        .collect::<Vec<_>>();
    assert_eq!(cells.len(), 2, "두 실제 셀이 각자의 소유 관계를 유지한다");
    assert_eq!(rendered_text(cells[0]), LABELS[0]);
    assert_eq!(rendered_text(cells[1]), "SpacedRightMarker");
    assert!(
        cells[0].bbox.x + cells[0].bbox.width < cells[1].bbox.x,
        "분리된 셀 상자 사이에 양의 간격이 있다"
    );
    for cell in cells {
        let RenderNodeType::TableCell(owner) = &cell.node_type else {
            unreachable!()
        };
        assert_eq!(
            owner.border_fill_id, border_fill_id,
            "가져오기가 참조를 보존한다"
        );
        assert!(
            cell.bbox.x > table.bbox.x
                && cell.bbox.y > table.bbox.y
                && cell.bbox.x + cell.bbox.width < table.bbox.x + table.bbox.width
                && cell.bbox.y + cell.bbox.height < table.bbox.y + table.bbox.height,
            "셀 상자가 네 방향 외곽 간격 안에 머문다"
        );
    }
    table
}

/// 관계형 합성 검사. 2026-10-29, 96 DPI로 측정한 원본 8쪽 표 간격 입력은
/// `borderFillIDRef=0`이고 테두리 정의가 없으며, 한컴과 upstream 반영 전 출력 모두 셀 변이 없다.
/// Native 회귀의 2~8쪽에서는 불필요한 선 8/16/12개를 관측했다. 수정 후 최종 측정은 대기 중이다.
///
/// 누락 스타일과 유효한 `Solid`/`None`, zone, 편집 가이드의 관계를 대조한다.
#[test]
fn spaced_cells_with_zero_or_unresolved_style_have_no_printable_edges() {
    use rhwp::paint::RenderProfile;
    for border_fill_id in [0, 99] {
        let core = open(&spaced_border_fixture(border_fill_id));
        let page = core.build_page_render_tree(0).expect("최종 렌더 트리");
        let table = spaced_border_table(&page.root, border_fill_id);
        assert!(
            border_nodes(table).is_empty(),
            "누락된 스타일이 자체 테두리를 추가하지 않는다"
        );
        let svg = core
            .render_page_svg_layer_with_profile_native(0, RenderProfile::Print)
            .expect("공개 API의 인쇄 SVG");
        assert!(
            !svg.contains("<line "),
            "인쇄 내보내기에 의도하지 않은 테두리가 없다"
        );
    }
}

#[test]
fn spaced_cells_keep_defined_solid_edges_and_explicit_none() {
    use rhwp::model::style::{BorderFill, BorderLine, BorderLineType};
    const COLOR: u32 = 0x123456;
    for (line_type, expected_edges) in [(BorderLineType::Solid, 8), (BorderLineType::None, 0)] {
        let mut source = spaced_border_fixture(1);
        source.doc_info.border_fills = vec![BorderFill {
            borders: [BorderLine {
                line_type,
                width: 3,
                color: COLOR,
            }; 4],
            ..Default::default()
        }];
        let core = open(&source);
        let page = core.build_page_render_tree(0).expect("최종 렌더 트리");
        let table = spaced_border_table(&page.root, 1);
        let lines = border_nodes(table);
        assert_eq!(
            lines.len(),
            expected_edges,
            "실제 셀마다 지정된 네 변을 유지한다"
        );
        for cell in table
            .children
            .iter()
            .filter(|node| matches!(node.node_type, RenderNodeType::TableCell(_)))
        {
            let (x, y, w, h) = (cell.bbox.x, cell.bbox.y, cell.bbox.width, cell.bbox.height);
            for (x1, y1, x2, y2) in [
                (x, y, x, y + h),
                (x + w, y, x + w, y + h),
                (x, y, x + w, y),
                (x, y + h, x + w, y + h),
            ] {
                let found = lines.iter().any(|node| {
                    matches!(&node.node_type,
                    RenderNodeType::Line(line) if !node.editor_only && line.style.color == COLOR
                        && (line.x1 - x1).abs() < 0.001 && (line.y1 - y1).abs() < 0.001
                        && (line.x2 - x2).abs() < 0.001 && (line.y2 - y2).abs() < 0.001)
                });
                assert_eq!(found, expected_edges > 0, "테두리가 최종 셀의 변을 따른다");
            }
        }
    }
}

#[test]
fn spaced_cellzone_keeps_defined_top_edges_above_missing_base_style() {
    use rhwp::model::style::{BorderFill, BorderLine, BorderLineType};
    use rhwp::model::table::TableZone;
    const COLOR: u32 = 0x654321;
    for border_fill_id in [0, 99] {
        let mut source = spaced_border_fixture(border_fill_id);
        let none = BorderLine {
            line_type: BorderLineType::None,
            ..Default::default()
        };
        source.doc_info.border_fills = vec![BorderFill {
            borders: [
                none,
                none,
                BorderLine {
                    line_type: BorderLineType::Solid,
                    width: 3,
                    color: COLOR,
                },
                none,
            ],
            ..Default::default()
        }];
        let Control::Table(table) = &mut source.sections[0].paragraphs[1].controls[0] else {
            panic!("간격이 있는 표의 소유 컨트롤")
        };
        table.zones.push(TableZone {
            start_col: 0,
            start_row: 0,
            end_col: 1,
            end_row: 0,
            border_fill_id: 1,
        });
        let core = open(&source);
        let page = core.build_page_render_tree(0).expect("최종 렌더 트리");
        let table = spaced_border_table(&page.root, border_fill_id);
        let lines = border_nodes(table);
        assert_eq!(lines.len(), 2, "zone이 정의된 두 위쪽 변만 제공한다");
        for cell in table
            .children
            .iter()
            .filter(|node| matches!(node.node_type, RenderNodeType::TableCell(_)))
        {
            assert!(
                lines.iter().any(|node| matches!(&node.node_type,
                RenderNodeType::Line(line) if !node.editor_only && line.style.color == COLOR
                    && (line.x1 - cell.bbox.x).abs() < 0.001
                    && (line.x2 - cell.bbox.x - cell.bbox.width).abs() < 0.001
                    && (line.y1 - cell.bbox.y).abs() < 0.001
                    && (line.y2 - cell.bbox.y).abs() < 0.001)),
                "zone 테두리가 분리된 각 셀의 위쪽 변을 따른다"
            );
        }
    }
}

#[test]
fn spaced_missing_style_guides_remain_editor_only_in_print_export() {
    use rhwp::paint::RenderProfile;
    use rhwp::wasm_api::HwpDocument;
    let bytes = rhwp::serializer::serialize_hwpx(&spaced_border_fixture(0)).expect("직렬화");
    let mut core = HwpDocument::from_bytes(&bytes).expect("공개 API로 가져오기");
    core.set_show_transparent_borders(true);
    let page = core.build_page_render_tree(0).expect("최종 편집기 트리");
    let table = spaced_border_table(&page.root, 0);
    let lines = border_nodes(table);
    assert_eq!(lines.len(), 8, "분리된 셀마다 가이드 네 변을 유지한다");
    assert!(
        lines.iter().all(|node| node.editor_only),
        "가이드는 인쇄하지 않는다"
    );
    let screen = core
        .render_page_svg_layer_with_profile_native(0, RenderProfile::Screen)
        .expect("화면 SVG");
    let print = core
        .render_page_svg_layer_with_profile_native(0, RenderProfile::Print)
        .expect("인쇄 SVG");
    assert!(
        screen.contains("<line "),
        "화면 내보내기가 표시된 가이드를 유지한다"
    );
    assert!(
        !print.contains("<line "),
        "인쇄 내보내기가 편집 가이드를 모두 제외한다"
    );
}

/// Relational synthetic check, 2026-10-29: 96-DPI render-tree boxes and authored
/// caption font sizes. No installed font metrics or raster reference are used.
#[test]
fn atomic_empty_caption_style_survives_initial_load_and_export_geometry() {
    use rhwp::model::shape::{Caption, CaptionDirection};
    for direction in [CaptionDirection::Top, CaptionDirection::Bottom] {
        for font_height in [200, 2000] {
            let mut source = fixture([BODY_WIDTH; 2], [7200; 2], ["", "", ""], 60000);
            source
                .doc_info
                .char_shapes
                .push(rhwp::model::style::CharShape {
                    base_size: font_height,
                    ..Default::default()
                });
            let mut empty = paragraph("", false);
            empty.char_shapes[0].char_shape_id = 1;
            let mut object = table(0, 40000, 7200);
            object.cells[0].paragraphs = vec![paragraph("한글 캡션 경계", true)];
            object.caption = Some(Caption {
                direction,
                spacing: 600,
                paragraphs: vec![empty],
                ..Default::default()
            });
            single_host(&mut source, object);
            let original = open(&source);
            let Control::Table(loaded) = &original.document().sections[0].paragraphs[1].controls[0]
            else {
                panic!("caption table owner")
            };
            let caption_lines = &loaded.caption.as_ref().unwrap().paragraphs[0].line_segs;
            assert!(
                !caption_lines.is_empty(),
                "initial load composes the missing caption row"
            );
            assert_eq!(caption_lines[0].line_height, font_height);
            // Build an independent input with those authored caption line metrics.
            // Its host still has no rows, so both inputs take the real load path.
            let Control::Table(authored) = &mut source.sections[0].paragraphs[1].controls[0] else {
                unreachable!()
            };
            authored.caption.as_mut().unwrap().paragraphs[0].line_segs = vec![LineSeg {
                line_height: font_height,
                text_height: font_height,
                baseline_distance: font_height * 85 / 100,
                segment_width: 40000,
                ..Default::default()
            }];
            let reference = open(&source);
            let reference_host = &reference.document().sections[0].paragraphs[1];
            assert_eq!(
                original.document().sections[0].paragraphs[1].line_segs[0].line_height,
                reference_host.line_segs[0].line_height,
                "missing-caption timing preserves authored occupied height: {direction:?}, {font_height}"
            );
            let exported = reopen(&original);
            for core in [&original, &exported] {
                let (tables, runs) = final_objects(core);
                assert_eq!(tables.len(), 1);
                let frame = tables[0].1;
                let tail = runs
                    .iter()
                    .find(|(text, _)| text.contains(AFTER))
                    .unwrap()
                    .1;
                // The authored caption row owns its physical height even when
                // its text is empty and produces no visible glyph run.
                let caption_height = px(font_height as u32);
                let bottom = frame.2
                    + frame.4
                    + if direction == CaptionDirection::Bottom {
                        px(600) + caption_height
                    } else {
                        0.0
                    };
                assert!(tail.0 > frame.0 || (tail.0 == frame.0 && tail.2 >= bottom - EPS),
                    "following clears the painted caption frame: {direction:?}, {font_height}, {frame:?}, {tail:?}");
                let (reference_tables, reference_runs) = final_objects(&reference);
                let reference_tail = reference_runs
                    .iter()
                    .find(|(text, _)| text.contains(AFTER))
                    .unwrap()
                    .1;
                assert!(
                    (frame.2 - reference_tables[0].1 .2).abs() < EPS,
                    "load and export retain caption/table origin: {direction:?}, {font_height}"
                );
                assert!((tail.2 - reference_tail.2).abs() < EPS,
                    "load and export retain following occupied origin: {direction:?}, {font_height}");
            }
        }
    }
}

#[test]
#[cfg(feature = "pdf-import")]
fn bounded_editor_reopen_rejects_unaccounted_atomic_hosts_and_seals_session() {
    use rhwp::{parser::pdf::PdfImportPolicy, PdfImportSession};
    let text_host = fixture([10000; 2], [7200; 2], ["한글", " 경계 ", "끝"], 60000);
    let mut empty_host = text_host.clone();
    let host = &mut empty_host.sections[0].paragraphs[1];
    host.text.clear();
    host.char_offsets.clear();
    host.char_count = 17;
    // This host fails the nonempty-text composer gate and exercises the planner gate.
    let mut nested_host = text_host.clone();
    let mut parent = table(0, 30000, 24000);
    parent.common.treat_as_char = false;
    parent.cells[0].paragraphs = vec![text_host.sections[0].paragraphs[1].clone()];
    single_host(&mut nested_host, parent);
    for source in [text_host, empty_host, nested_host] {
        let bytes = rhwp::serializer::serialize_hwpx(&source).unwrap();
        // The ordinary document API retains the independent native geometry path.
        let ordinary = open(&source);
        assert!(!final_objects(&ordinary).0.is_empty());
        let mut request = PdfImportSession::new(PdfImportPolicy::default()).unwrap();
        let error = request
            .reopen_hwpx_export(&bytes, 1024 * 1024)
            .err()
            .expect("generated-editor boundary must reject unsupported atomic geometry")
            .to_string();
        assert!(
            error.contains("PDF_EDITOR_REOPEN: unsupported atomic table host"),
            "{error}"
        );
        assert!(
            !error.contains("PDF_RESOURCE_LIMIT:"),
            "unsupported geometry is not budget exhaustion"
        );
        assert!(request.resource_usage().failure.is_none());
        let used = request.resource_usage().decoded_bytes;
        assert!(request.reopen_hwpx_export(&bytes, 1024 * 1024).is_err());
        assert_eq!(request.resource_usage().decoded_bytes, used);
        let mut policy = PdfImportPolicy::default();
        policy.backend.decoded_bytes = bytes.len();
        let mut limited = PdfImportSession::new(policy).unwrap();
        let error = limited
            .reopen_hwpx_export(&bytes, 1024 * 1024)
            .err()
            .unwrap()
            .to_string();
        assert!(error.contains("PDF_RESOURCE_LIMIT:"), "{error}");
        assert!(limited.resource_usage().failure.is_some());
        assert!(limited.resource_usage().decoded_bytes <= policy.backend.decoded_bytes);
    }
}

#[test]
#[cfg(feature = "pdf-import")]
fn bounded_editor_reopen_keeps_empty_stored_nested_tac_hosts() {
    use rhwp::{parser::pdf::PdfImportPolicy, PdfImportSession};
    let mut source = fixture([20000; 2], [12000; 2], ["", "", ""], 60000);
    let mut parent = table(0, 20000, 12000);
    parent.common.treat_as_char = false;
    let child = table(0, 10000, 7200);
    let mut host = paragraph("", true);
    host.char_count += 8;
    host.line_segs[0].line_height = 7200;
    host.line_segs[0].text_height = 7200;
    host.line_segs[0].baseline_distance = 6120;
    host.controls = vec![Control::Table(Box::new(child))];
    parent.cells[0].paragraphs = vec![host];
    single_host(&mut source, parent);
    let bytes = rhwp::serializer::serialize_hwpx(&source).unwrap();
    let mut request = PdfImportSession::new(PdfImportPolicy::default()).unwrap();
    let core = request.reopen_hwpx_export(&bytes, 1024 * 1024).unwrap();
    let Control::Table(parent) = &core.document().sections[0].paragraphs[1].controls[0] else {
        panic!("parent")
    };
    let host = &parent.cells[0].paragraphs[0];
    assert!(host.text.is_empty());
    assert!(!host.line_segs.is_empty());
    assert!(host
        .line_segs
        .iter()
        .all(|line| line.tag & LineSeg::TAG_IMPLEMENTATION_PROPERTY == 0));
    assert!(matches!(&host.controls[0], Control::Table(child) if child.common.treat_as_char));
    assert!(!core.export_hwpx_native().unwrap().is_empty());
    assert!(!core
        .build_page_render_tree(0)
        .unwrap()
        .root
        .children
        .is_empty());
}

#[test]
#[cfg(feature = "pdf-import")]
fn bounded_editor_reopen_keeps_actual_generated_pdf_handoffs() {
    use rhwp::{parser::pdf::PdfImportPolicy, PdfImportSession};
    for pdf in [
        include_bytes!("../../pdf/issue8174/fonts/cff-cid.pdf").as_slice(),
        include_bytes!("../../pdf/issue7998/cell-nested.pdf").as_slice(),
    ] {
        let mut import = PdfImportSession::new(PdfImportPolicy::default()).unwrap();
        let mut original = import.open(pdf, &Default::default()).unwrap();
        let handoff = import.export_hwpx(&mut original, 1024 * 1024).unwrap();
        let mut request = PdfImportSession::new(PdfImportPolicy::default()).unwrap();
        let reopened = request.reopen_hwpx_export(&handoff, 1024 * 1024).unwrap();
        assert_eq!(reopened.page_count(), original.page_count());
        for page in 0..reopened.page_count() {
            assert!(!reopened
                .build_page_render_tree(page)
                .unwrap()
                .root
                .children
                .is_empty());
        }
        assert!(!reopened.export_hwpx_native().unwrap().is_empty());
    }
}
