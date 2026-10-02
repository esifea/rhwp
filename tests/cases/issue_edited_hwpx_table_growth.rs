//! Cache-free edited HWPX cells must occupy their complete fresh line boxes.
//! These are synthetic layout contracts, not Hancom saved-output oracles.
#![cfg(not(target_arch = "wasm32"))]

use rhwp::document_core::DocumentCore;
use rhwp::model::control::Control;
use rhwp::model::document::{Document, Section, SectionDef};
use rhwp::model::page::PageDef;
use rhwp::model::paragraph::{CharShapeRef, LineSeg, Paragraph};
use rhwp::model::table::{Cell, Table, TablePageBreak};
use rhwp::renderer::render_tree::{PageRenderTree, RenderNode, RenderNodeType};

const EDIT: &str = "Long wrapped synthetic content Long wrapped synthetic content Long wrapped synthetic content";
const AFTER: &str = "Following body marker";
const DPI: f64 = 96.0;

fn paragraph(text: &str, cached: bool) -> Paragraph {
    Paragraph {
        text: text.into(),
        char_count: text.encode_utf16().count() as u32 + 1,
        char_offsets: (0..text.encode_utf16().count() as u32).collect(),
        char_shapes: vec![CharShapeRef {
            start_pos: 0,
            char_shape_id: 0,
        }],
        line_segs: if cached {
            vec![LineSeg {
                line_height: 1200,
                text_height: 1200,
                baseline_distance: 1000,
                line_spacing: 300,
                segment_width: 15000,
                ..Default::default()
            }]
        } else {
            Vec::new()
        },
        ..Default::default()
    }
}

fn fixture(
    rows: u16,
    row_height: u32,
    edits: usize,
    all_fresh: bool,
    merged: bool,
    split: bool,
    small_page: bool,
) -> Document {
    let mut table = Table {
        row_count: rows,
        col_count: if merged { 3 } else { 2 },
        page_break: if split {
            TablePageBreak::RowBreak
        } else {
            TablePageBreak::None
        },
        ..Default::default()
    };
    table.common.width = 30000;
    table.common.height = row_height * u32::from(rows);
    table.common.treat_as_char = true;
    table.common.flow_with_text = true;
    table.common.text_wrap = rhwp::model::shape::TextWrap::TopAndBottom;
    table.common.vert_rel_to = rhwp::model::shape::VertRelTo::Para;
    table.padding.top = 150;
    table.padding.bottom = 150;
    for row in 0..rows {
        let edited = usize::from(row) > 0 && usize::from(row) <= edits;
        table.cells.push(Cell {
            row,
            col: 0,
            width: 15000,
            height: row_height,
            row_span: 1,
            col_span: 1,
            paragraphs: vec![paragraph(&format!("Label{row}"), !all_fresh)],
            ..Default::default()
        });
        if merged && row == 2 {
            continue;
        }
        table.cells.push(Cell {
            row,
            col: 1,
            width: 15000,
            height: if merged && row == 1 {
                row_height * 2
            } else {
                row_height
            },
            row_span: if merged && row == 1 { 2 } else { 1 },
            col_span: if merged { 2 } else { 1 },
            paragraphs: vec![paragraph(
                if edited { EDIT } else { "Value" },
                !all_fresh && !edited,
            )],
            ..Default::default()
        });
    }
    table.rebuild_grid();
    let mut host = paragraph("", edits == 0 && !all_fresh);
    if let Some(seg) = host.line_segs.first_mut() {
        seg.line_height = table.common.height as i32;
    }
    host.controls.push(Control::Table(Box::new(table)));
    document(vec![host, paragraph(AFTER, true)], small_page)
}

fn document(paragraphs: Vec<Paragraph>, small_page: bool) -> Document {
    let mut doc = Document::default();
    doc.doc_info.char_shapes = vec![rhwp::model::style::CharShape {
        base_size: 1000,
        ..Default::default()
    }];
    doc.doc_info.para_shapes = vec![rhwp::model::style::ParaShape {
        line_spacing: 160,
        ..Default::default()
    }];
    doc.doc_info.styles = vec![Default::default()];
    let mut page_def = PageDef::a4_default();
    if small_page {
        page_def.height = 30000;
        page_def.margin_top = 1500;
        page_def.margin_bottom = 1500;
        page_def.margin_header = 0;
        page_def.margin_footer = 0;
    }
    doc.sections.push(Section {
        section_def: SectionDef {
            page_def,
            ..Default::default()
        },
        paragraphs,
        ..Default::default()
    });
    doc
}

fn open(source: &Document) -> DocumentCore {
    let bytes = rhwp::serializer::serialize_hwpx(source).expect("serialize synthetic HWPX");
    let core = DocumentCore::from_bytes(&bytes).expect("open synthetic HWPX through public API");
    assert_cache_state(source, &core);
    core
}

fn visit<'a>(node: &'a RenderNode, out: &mut Vec<&'a RenderNode>) {
    out.push(node);
    for child in &node.children {
        visit(child, out);
    }
}

fn trees(core: &DocumentCore) -> Vec<PageRenderTree> {
    (0..core.page_count())
        .map(|page| {
            core.build_page_render_tree(page)
                .expect("build actual page tree")
        })
        .collect()
}

fn compact(text: &str) -> String {
    text.chars().filter(|c| !c.is_whitespace()).collect()
}

fn table(source: &Document) -> &Table {
    source.sections[0].paragraphs[0]
        .controls
        .iter()
        .find_map(|control| {
            if let Control::Table(table) = control {
                Some(table.as_ref())
            } else {
                None
            }
        })
        .expect("table owner")
}

fn assert_source_properties(source: &Document, core: &DocumentCore) {
    let before = table(source);
    let after = table(core.document());
    assert_eq!(before.common.width, after.common.width);
    assert_eq!(before.common.height, after.common.height);
    assert_eq!(before.page_break, after.page_break);
    assert_eq!(before.cells.len(), after.cells.len());
    for (a, b) in before.cells.iter().zip(&after.cells) {
        assert_eq!(
            (a.row, a.col, a.row_span, a.col_span, a.width, a.height),
            (b.row, b.col, b.row_span, b.col_span, b.width, b.height)
        );
        assert_eq!(a.paragraphs[0].text, b.paragraphs[0].text);
        assert_eq!(a.paragraphs[0].para_shape_id, b.paragraphs[0].para_shape_id);
        assert_eq!(
            a.paragraphs[0].char_shapes[0].char_shape_id,
            b.paragraphs[0].char_shapes[0].char_shape_id
        );
    }
}

fn assert_geometry(source: &Document, core: &DocumentCore, require_body_bounds: bool) -> f64 {
    assert_source_properties(source, core);
    let mut cell_text = String::new();
    let mut expected_text = String::new();
    for cell in &table(source).cells {
        expected_text.push_str(&cell.paragraphs[0].text);
    }
    let mut actual_units = Vec::new();
    let mut after_positions = Vec::new();
    let mut table_positions = Vec::new();
    let mut table_height = 0.0;
    let body_bottom = f64::from(
        source.sections[0].section_def.page_def.height - source.sections[0].section_def.page_def.margin_bottom,
    ) * DPI / 7200.0;
    for (page, tree) in trees(core).iter().enumerate() {
        let mut nodes = Vec::new();
        visit(&tree.root, &mut nodes);
        for node in nodes {
            match &node.node_type {
                RenderNodeType::Table(_) => {
                    table_height += node.bbox.height;
                    table_positions.push((page, node.bbox.y + node.bbox.height));
                    if require_body_bounds {
                        assert!(
                            node.bbox.y + node.bbox.height <= body_bottom + 0.5,
                            "table fragment extends below body on page {page}: {:?}",
                            node.bbox
                        );
                    }
                }
                RenderNodeType::TableCell(_) => {
                    let mut descendants = Vec::new();
                    visit(node, &mut descendants);
                    for child in descendants {
                        if let RenderNodeType::TextLine(_) = &child.node_type {
                            assert!(
                                child.bbox.y >= node.bbox.y - 0.5
                                    && child.bbox.y + child.bbox.height
                                        <= node.bbox.y + node.bbox.height + 0.5,
                                "line {:?} does not fit owning cell {:?} on page {page}",
                                child.bbox,
                                node.bbox
                            );
                        }
                        if let RenderNodeType::TextRun(run) = &child.node_type {
                            cell_text.push_str(&run.text);
                            actual_units.extend(compact(&run.text).chars());
                        }
                    }
                }
                RenderNodeType::TextRun(run) if run.text.contains(AFTER) => {
                    after_positions.push((page, node.bbox.y));
                    assert!(
                        node.bbox.y + node.bbox.height <= body_bottom + 0.5,
                        "following body text outside page"
                    );
                }
                _ => {}
            }
        }
    }
    let mut expected_units: Vec<_> = compact(&expected_text).chars().collect();
    expected_units.sort_unstable();
    actual_units.sort_unstable();
    assert_eq!(
        actual_units, expected_units,
        "lost or duplicated cell text units: {cell_text}"
    );
    assert_eq!(
        after_positions.len(),
        1,
        "following body must appear exactly once"
    );
    let after = after_positions[0];
    let last = *table_positions.last().expect("table fragment");
    assert!(
        after.0 > last.0 || (after.0 == last.0 && after.1 >= last.1 - 0.5),
        "following body {after:?} overlaps table end {last:?}"
    );
    table_height
}

#[test]
fn issue_edited_hwpx_table_growth_mixed_cached_siblings_fit() {
    let source = fixture(3, 1800, 1, false, false, false, false);
    let core = open(&source);
    assert_geometry(&source, &core, true);
}

#[test]
fn issue_edited_hwpx_table_growth_all_uncached_cumulative_exceeds_48px() {
    let source = fixture(18, 1300, 2, true, false, false, false);
    let core = open(&source);
    let height = assert_geometry(&source, &core, true);
    let declared = f64::from(table(&source).common.height) * DPI / 7200.0;
    assert!(
        height > declared + 48.0,
        "fixture must exercise cumulative growth above old cap: {height} vs {declared}"
    );
}

#[test]
fn issue_edited_hwpx_table_growth_rowspan_and_horizontal_merge_fit() {
    let source = fixture(3, 1800, 1, false, true, false, false);
    let core = open(&source);
    assert_geometry(&source, &core, true);
}

#[test]
fn issue_edited_hwpx_table_growth_cell_break_flows_across_page_boundary() {
    let source = fixture(18, 1300, 2, true, false, true, true);
    let core = open(&source);
    let expected_page = &source.sections[0].section_def.page_def;
    let loaded_page = &core.document().sections[0].section_def.page_def;
    assert_eq!(
        (
            expected_page.height,
            expected_page.margin_top,
            expected_page.margin_bottom
        ),
        (
            loaded_page.height,
            loaded_page.margin_top,
            loaded_page.margin_bottom
        ),
        "small page must survive HWPX reopen"
    );
    assert_geometry(&source, &core, true);
    assert!(
        core.page_count() >= 2,
        "fresh row growth needs another page"
    );
}

#[test]
fn issue_edited_hwpx_table_growth_unchanged_cached_control() {
    let source = fixture(3, 1800, 0, false, false, false, false);
    let core = open(&source);
    assert_eq!(core.page_count(), 1);
    let height = assert_geometry(&source, &core, true);
    assert!(
        (height - 72.0).abs() < 0.5,
        "saved 5400HU table must keep its height: {height}"
    );
}

const SYNTHETIC: u32 = LineSeg::TAG_IMPLEMENTATION_PROPERTY;

#[derive(Clone, Copy)]
enum Text {
    Cached(&'static str),
    Fresh(&'static str),
}

struct TableSpan {
    page: usize,
    nested: bool,
    top: f64,
    bottom: f64,
}

#[derive(Default)]
struct Walk {
    violations: Vec<String>,
    /// Compact text of every run inside a cell, in page then paint order.
    cell_text: String,
    tables: Vec<TableSpan>,
    /// Painted height of each cell and whether any text landed in it.
    cells: Vec<(f64, bool)>,
    /// Page, top and bottom of each following-body run.
    after: Vec<(usize, f64, f64)>,
    page_has_text: Vec<bool>,
}

fn shell(rows: u16, cols: u16, width: u32, height: u32, page_break: TablePageBreak) -> Table {
    let mut table = Table {
        row_count: rows,
        col_count: cols,
        page_break,
        ..Default::default()
    };
    table.common.width = width;
    table.common.height = height;
    table.common.treat_as_char = true;
    table.common.flow_with_text = true;
    table.common.text_wrap = rhwp::model::shape::TextWrap::TopAndBottom;
    table.common.vert_rel_to = rhwp::model::shape::VertRelTo::Para;
    table.padding.top = 150;
    table.padding.bottom = 150;
    table
}

fn cell_at(row: u16, col: u16, width: u32, height: u32, content: Paragraph) -> Cell {
    Cell {
        row,
        col,
        width,
        height,
        row_span: 1,
        col_span: 1,
        paragraphs: vec![content],
        ..Default::default()
    }
}

/// A paragraph owning `table`, whose saved line keeps the table's declared height.
fn host(table: Table, cached: bool) -> Paragraph {
    let mut owner = paragraph("", cached);
    if let Some(seg) = owner.line_segs.first_mut() {
        seg.line_height = table.common.height as i32;
    }
    owner.controls.push(Control::Table(Box::new(table)));
    owner
}

/// Two 15000HU columns; each row and the table declare their own heights.
/// The section host keeps its saved line only when no cell is fresh.
fn grid(row_heights: &[u32], height: u32, state: impl Fn(u16, u16) -> Text) -> Document {
    let rows = row_heights.len() as u16;
    let mut table = shell(rows, 2, 30000, height, TablePageBreak::None);
    let mut saved = true;
    for (row, &row_height) in (0..rows).zip(row_heights) {
        for col in 0..2 {
           let content = match state(row, col) {
                Text::Cached(text) => paragraph(text, true),
                Text::Fresh(text) => {
                    saved = false;
                    paragraph(text, false)
                }
            };
            table.cells.push(cell_at(row, col, 15000, row_height, content));
        }
    }
    table.rebuild_grid();
    document(vec![host(table, saved), paragraph(AFTER, true)], false)
}

/// A real outer row owns nested fresh content and a saved sibling cell.
/// Its 4200HU table frame is shorter than the fresh nested content plus padding.
fn nested_fixture() -> Document {
    let mut inner = shell(1, 1, 15000, 3600, TablePageBreak::None);
    inner.cells.push(cell_at(0, 0, 15000, 3600, paragraph(EDIT, false)));
    inner.rebuild_grid();

    let mut outer = shell(1, 2, 32000, 4200, TablePageBreak::None);
    outer.cells.push(cell_at(0, 0, 16000, 5000, host(inner, true)));
    outer.cells.push(cell_at(0, 1, 16000, 5000, paragraph("Outer label", true)));
    outer.rebuild_grid();
    document(vec![host(outer, false), paragraph(AFTER, true)], false)
}

fn long_text() -> String {
    (1..=200).map(|n| format!("segment{n:03}")).collect::<Vec<_>>().join(" ")
}

/// One saved label and one fresh cell far taller than the small page body.
fn long_cell_fixture() -> Document {
    let mut table = shell(1, 2, 30000, 1800, TablePageBreak::CellBreak);
    table.cells.push(cell_at(0, 0, 15000, 1800, paragraph("Label0", true)));
    table.cells.push(cell_at(0, 1, 15000, 1800, paragraph(&long_text(), false)));
    table.rebuild_grid();
    document(vec![host(table, false), paragraph(AFTER, true)], true)
}

fn is_fresh(paragraph: &Paragraph) -> bool {
    paragraph.line_segs.is_empty()
        || paragraph
            .line_segs
            .iter()
            .all(|seg| seg.tag & SYNTHETIC != 0)
}

fn collect<'a>(table: &'a Table, out: &mut Vec<&'a Paragraph>) {
    for paragraph in table.cells.iter().flat_map(|cell| &cell.paragraphs) {
        out.push(paragraph);
        for control in &paragraph.controls {
            if let Control::Table(nested) = control {
                collect(nested, out);
            }
        }
    }
}

// A saved paragraph must reopen with a real cache and a fresh one without, or the fixtures
// stop testing the saved versus fresh contract. Nested tables are walked too.
fn assert_cache_state(source: &Document, core: &DocumentCore) {
    let (mut before, mut after) = (Vec::new(), Vec::new());
    collect(table(source), &mut before);
    collect(table(core.document()), &mut after);
    assert_eq!(
        before.len(),
        after.len(),
        "paragraph count changed on reopen"
    );
    for (a, b) in before.iter().zip(&after) {
        assert_eq!(
            is_fresh(a),
            is_fresh(b),
            "cache state changed on reopen for {:?}",
            a.text
        );
        if !is_fresh(a) {
            assert_eq!(
                a.line_segs[0].line_height, b.line_segs[0].line_height,
                "saved line height changed on reopen for {:?}",
                a.text
            );
        }
    }
}

fn escapes(span: (f64, f64), owner: Option<(f64, f64)>) -> bool {
    owner.is_some_and(|cell| span.0 < cell.0 - 0.5 || span.1 > cell.1 + 0.5)
}

/// Records every painted table, cell and run. Lines and nested tables must sit inside the
/// nearest owning cell; breaches are collected so a probe can still print them.
fn walk(node: &RenderNode, page: usize, owner: Option<(f64, f64)>, out: &mut Walk) {
    let span = (node.bbox.y, node.bbox.y + node.bbox.height);
    let mut inner = owner;
    match &node.node_type {
        RenderNodeType::Table(_) => {
            if escapes(span, owner) {
                out.violations.push(format!(
                    "nested table {span:?} leaves owning cell {owner:?} on page {page}"
                ));
            }
            out.tables.push(TableSpan {
                page,
                nested: owner.is_some(),
                top: span.0,
                bottom: span.1,
            });
        }
        RenderNodeType::TableCell(_) => inner = Some(span),
        RenderNodeType::TextLine(_) if escapes(span, owner) => {
            out.violations.push(format!(
                "line {span:?} does not fit owning cell {owner:?} on page {page}"
            ));
        }
        RenderNodeType::TextRun(run) => {
            let text = compact(&run.text);
            out.page_has_text[page] |= !text.is_empty();
            if owner.is_some() {
                out.cell_text.push_str(&text);
            } else if run.text.contains(AFTER) {
                out.after.push((page, span.0, span.1));
            }
        }
        _ => {}
    }
    let start = out.cell_text.len();
    for child in &node.children {
        walk(child, page, inner, out);
    }
    if matches!(&node.node_type, RenderNodeType::TableCell(_)) {
        out.cells
            .push((node.bbox.height, out.cell_text.len() > start));
    }
}

fn paint(core: &DocumentCore) -> Walk {
    let trees = trees(core);
    let mut out = Walk {
        page_has_text: vec![false; trees.len()],
        ..Default::default()
    };
    for (page, tree) in trees.iter().enumerate() {
        walk(&tree.root, page, None, &mut out);
    }
    out
}

fn assert_flow(source: &Document, painted: &Walk) {
    assert!(
        painted.violations.is_empty(),
        "{}",
        painted.violations.join("; ")
    );
    let page_def = &source.sections[0].section_def.page_def;
    let body_bottom = f64::from(page_def.height - page_def.margin_bottom) * DPI / 7200.0;
    let outer: Vec<_> = painted.tables.iter().filter(|span| !span.nested).collect();
    for span in &outer {
        assert!(
            span.bottom <= body_bottom + 0.5,
            "table fragment extends below body on page {}: {}",
            span.page,
            span.bottom
        );
    }
    assert_eq!(
        painted.after.len(),
        1,
        "following body must appear exactly once"
    );
    let (page, top, bottom) = painted.after[0];
    assert!(
        bottom <= body_bottom + 0.5,
        "following body text outside page"
    );
    let last = outer.last().expect("table fragment");
    assert!(
        page > last.page || (page == last.page && top >= last.bottom - 0.5),
        "following body at page {page} y {top} overlaps table end at page {} y {}",
        last.page,
        last.bottom
    );
}

#[test]
fn issue_edited_hwpx_table_growth_nested_fresh_table_stays_inside_saved_outer_cell() {
    let source = nested_fixture();
    let core = open(&source);
    let painted = paint(&core);
    assert_flow(&source, &painted);
    assert!(
        painted.tables.iter().any(|span| span.nested),
        "nested table must be painted"
    );
    assert_eq!(
        painted.cell_text,
        compact(&format!("{EDIT}Outer label")),
        "nested cell text lost or duplicated"
    );
}

#[test]
fn issue_edited_hwpx_table_growth_cached_declared_contradiction_keeps_table_height() {
    let source = grid(&[2400; 3], 6600, |_, _| Text::Cached("Value"));
    let core = open(&source);
    let painted = paint(&core);
    assert_flow(&source, &painted);
    let declared = f64::from(table(&source).common.height) * DPI / 7200.0;
    let height = painted.tables[0].bottom - painted.tables[0].top;
    assert!(
        (height - declared).abs() < 0.5,
        "saved rows above the table height must still fit it: {height} vs {declared}"
    );
}

#[test]
fn issue_edited_hwpx_table_growth_empty_cacheless_cells_keep_their_line_box() {
    let source = grid(&[1800, 800, 800], 3400, |row, _| {
        if row == 0 {
            Text::Cached("Label")
        } else {
            Text::Fresh("")
        }
    });
    let core = open(&source);
    let painted = paint(&core);
    assert_flow(&source, &painted);
    // An empty paragraph still owns one 10pt line between the table's 150HU paddings.
    let line_box = f64::from(1000 + 150 + 150) * DPI / 7200.0;
    let saved_row = f64::from(1800) * DPI / 7200.0;
    let height = painted.tables[0].bottom - painted.tables[0].top;
    assert!(
        height >= saved_row + 2.0 * line_box - 0.5,
        "empty rows collapsed to their declared 800HU: {height}"
    );
    for (cell_height, has_text) in &painted.cells {
        assert!(
            *has_text || *cell_height >= line_box - 0.5,
            "empty cell {cell_height} is below its line box {line_box}"
        );
    }
}

#[test]
fn issue_edited_hwpx_table_growth_synthetic_tagged_cache_form_fits() {
    let mut source = fixture(3, 1800, 1, false, false, false, false);
    let Control::Table(table) = &mut source.sections[0].paragraphs[0].controls[0] else {
        panic!("host paragraph must own the table");
    };
    for paragraph in table.cells.iter_mut().flat_map(|cell| &mut cell.paragraphs) {
        if paragraph.line_segs.is_empty() {
            paragraph.line_segs.push(LineSeg {
                line_height: 1200,
                text_height: 1200,
                baseline_distance: 1000,
                line_spacing: 300,
                segment_width: 15000,
                tag: SYNTHETIC,
                ..Default::default()
            });
        }
    }
    let core = open(&source);
    assert_geometry(&source, &core, true);
}

#[test]
fn issue_edited_hwpx_table_growth_stale_following_cache_stays_below_table() {
    let mut source = fixture(3, 1800, 1, false, false, false, false);
    // The following paragraph keeps the position it had before the edit grew the table.
    let saved_bottom = table(&source).common.height as i32;
    source.sections[0].paragraphs[1].line_segs[0].vertical_pos = saved_bottom;
    let core = open(&source);
    assert_geometry(&source, &core, true);
}

#[test]
fn issue_edited_hwpx_table_growth_cellbreak_mode_continues_long_cell_across_pages() {
    let source = long_cell_fixture();
    let core = open(&source);
    let painted = paint(&core);
    assert_flow(&source, &painted);
    assert!(
        core.page_count() >= 3,
        "the long cell needs a middle fragment with both a start and an end cut"
    );
    assert!(
        painted.page_has_text.iter().all(|&has_text| has_text),
        "empty page among {:?}",
        painted.page_has_text
    );
    assert_eq!(
        painted.cell_text,
        compact(&format!("Label0{}", long_text())),
        "cell text lost, duplicated or reordered across fragments"
    );
}

#[test]
fn issue_edited_hwpx_table_growth_cacheless_declared_contradiction_keeps_table_height() {
    let mut source = grid(&[2400; 3], 6600, |_, _| Text::Fresh("Value"));
    source.sections[0].paragraphs[0] = host(table(&source).clone(), true);
    let core = open(&source);
    let painted = paint(&core);
    assert_flow(&source, &painted);
    let height = painted.tables[0].bottom - painted.tables[0].top;
    assert!(
        (height - 88.0).abs() <= 0.5,
        "short fresh content fits the declared 6600HU frame: {height}"
    );
}

#[test]
fn issue_edited_hwpx_table_growth_empty_cacheless_cell_keeps_saved_table_frame() {
    let mut source = grid(&[2400; 3], 6600, |row, col| {
        if (row, col) == (1, 1) {
            Text::Fresh("")
        } else {
            Text::Cached("Value")
        }
    });
    source.sections[0].paragraphs[0] = host(table(&source).clone(), true);
    let core = open(&source);
    let painted = paint(&core);
    assert_flow(&source, &painted);
    let height = painted.tables[0].bottom - painted.tables[0].top;
    assert!(
        (height - 88.0).abs() <= 0.5,
        "one empty uncached line fits the saved 6600HU frame: {height}"
    );
}

mod host_flow_review {
    //! Independent synthetic review probes. Manual stored metadata is a contract input,
    //! not a Hancom saved-output oracle. Public HWPX roundtrip and final render bounds.
    use rhwp::document_core::DocumentCore;
    use rhwp::model::control::Control;
    use rhwp::model::document::{Document, Section, SectionDef};
    use rhwp::model::page::PageDef;
    use rhwp::model::paragraph::{CharShapeRef, LineSeg, Paragraph};
    use rhwp::model::table::{Cell, Table, TablePageBreak};
    use rhwp::renderer::render_tree::{RenderNode, RenderNodeType};

    const AFTER: &str = "FollowingBodyProbe";
    const WIDTH: i32 = 42520;
    const DECLARED: i32 = 5400;
    fn text(value: &str) -> Paragraph {
        Paragraph {
            text: value.into(),
            char_count: value.encode_utf16().count() as u32 + 1,
            char_offsets: (0..value.encode_utf16().count() as u32).collect(),
            char_shapes: vec![CharShapeRef {
                start_pos: 0,
                char_shape_id: 0,
            }],
            ..Default::default()
        }
    }
    fn line(start: u32, top: i32, height: i32, table: bool) -> LineSeg {
        LineSeg {
            text_start: start,
            vertical_pos: top,
            line_height: height,
            text_height: height,
            baseline_distance: if table { height } else { 1000 },
            segment_width: WIDTH,
            tag: LineSeg::TAG_SINGLE_SEGMENT_LINE,
            ..Default::default()
        }
    }
    fn fresh_table() -> Table {
        let mut table = Table {
            row_count: 3,
            col_count: 2,
            page_break: TablePageBreak::None,
            ..Default::default()
        };
        table.common.width = WIDTH as u32;
        table.common.height = DECLARED as u32;
        table.common.treat_as_char = true;
        table.common.flow_with_text = true;
        table.common.text_wrap = rhwp::model::shape::TextWrap::TopAndBottom;
        table.common.vert_rel_to = rhwp::model::shape::VertRelTo::Para;
        table.padding.top = 150;
        table.padding.bottom = 150;
        for row in 0..3 {
            for col in 0..2 {
                table.cells.push(Cell {
                    row,
                    col,
                    row_span: 1,
                    col_span: 1,
                    width: WIDTH as u32 / 2,
                    height: 1800,
                    paragraphs: vec![text("Fresh first line"), text("Fresh second line")],
                    ..Default::default()
                });
            }
        }
        table.rebuild_grid();
        table
    }
    fn open(host: Paragraph) -> DocumentCore {
        open_with_body_height(host, None)
    }
    fn open_with_body_height(host: Paragraph, body_height_hu: Option<u32>) -> DocumentCore {
        let mut doc = Document::default();
        doc.doc_info.char_shapes = vec![rhwp::model::style::CharShape {
            base_size: 1000,
            ..Default::default()
        }];
        doc.doc_info.para_shapes = vec![rhwp::model::style::ParaShape {
            line_spacing: 160,
            ..Default::default()
        }];
        doc.doc_info.styles = vec![Default::default()];
        let mut page_def = PageDef::a4_default();
        if let Some(body) = body_height_hu {
            page_def.margin_top = 1000;
            page_def.margin_bottom = 1000;
            page_def.margin_header = 0;
            page_def.margin_footer = 0;
            page_def.height = body + 2000;
        }
        doc.sections.push(Section {
            section_def: SectionDef {
                page_def,
                ..Default::default()
            },
            paragraphs: vec![host, text(AFTER)],
            ..Default::default()
        });
        let bytes = rhwp::serializer::serialize_hwpx(&doc).expect("serialize review fixture");
        DocumentCore::from_bytes(&bytes).expect("public HWPX reopen")
    }
    fn visit<'a>(node: &'a RenderNode, out: &mut Vec<&'a RenderNode>) {
        out.push(node);
        for child in &node.children {
            visit(child, out);
        }
    }
    fn assert_following_clears_host(core: &DocumentCore, expected_tables: usize, host_text: &str) {
        let host = &core.document().sections[0].paragraphs[0];
        assert!(
            !host.line_segs.is_empty(),
            "saved host cache is part of this contract"
        );
        for control in &host.controls {
            if let Control::Table(table) = control {
                assert_eq!(table.page_break, TablePageBreak::None);
                assert!(
                    table
                        .cells
                        .iter()
                        .flat_map(|c| &c.paragraphs)
                        .all(|p| p.line_segs.is_empty()
                            || p.line_segs.iter().all(|s| s.tag & 0x8000_0000 != 0)),
                    "fresh cell cache state"
                );
            }
        }
        let page_def = &core.document().sections[0].section_def.page_def;
        let body_bottom = f64::from(page_def.height - page_def.margin_bottom) / 75.0;
        let mut host_end: Option<(u32, f64)> = None;
        let mut following = Vec::new();
        let mut rendered_host = String::new();
        let mut table_count = 0;
        for page in 0..core.page_count() {
            let tree = core.build_page_render_tree(page).expect("actual page tree");
            let mut nodes = Vec::new();
            visit(&tree.root, &mut nodes);
            for node in nodes {
                let occupied = match &node.node_type {
                    RenderNodeType::Table(_) => {
                        table_count += 1;
                        assert!(
                            node.bbox.y + node.bbox.height <= body_bottom + 0.5,
                            "whole NONE table outside body: {:?}, bottom={body_bottom}",
                            node.bbox
                        );
                        true
                    }
                    RenderNodeType::TextRun(run)
                        if run.para_index == Some(0) && run.cell_context.is_none() =>
                    {
                        rendered_host.push_str(&run.text);
                        true
                    }
                    RenderNodeType::TextRun(run) if run.text.contains(AFTER) => {
                        following.push((page, node.bbox.y));
                        assert!(
                            node.bbox.y + node.bbox.height <= body_bottom + 0.5,
                            "following body outside actual page body: {:?}, bottom={body_bottom}",
                            node.bbox
                        );
                        false
                    }
                    _ => false,
                };
                if occupied {
                    let end = (page, node.bbox.y + node.bbox.height);
                    if host_end.is_none_or(|old| end.0 > old.0 || (end.0 == old.0 && end.1 > old.1))
                    {
                        host_end = Some(end);
                    }
                }
            }
        }
        assert_eq!(table_count, expected_tables, "NONE keeps each table whole");
        assert_eq!(
            rendered_host.replace('\n', ""),
            host_text.replace('\n', ""),
            "host text preserved exactly once"
        );
        assert_eq!(following.len(), 1, "following body preserved exactly once");
        let after = following[0];
        let end = host_end.expect("actual occupied host bottom");
        eprintln!("review host flow: occupied end={end:?}; following={after:?}");
        assert!(
            after.0 > end.0 || (after.0 == end.0 && after.1 >= end.1 - 0.5),
            "following {after:?} overlaps actual host/table end {end:?}"
        );
    }
    #[test]
    fn review_saved_host_before_grown_none_table() {
        let mut host = text("BeforeProbe");
        let len = host.text.len() as u32;
        host.char_count += 8;
        host.controls = vec![Control::Table(Box::new(fresh_table()))];
        host.line_segs = vec![line(0, 0, 1200, false), line(len, 1200, DECLARED, true)];
        assert_following_clears_host(&open(host), 1, "BeforeProbe");
    }
    #[test]
    fn review_saved_host_after_grown_none_table() {
        let mut host = text("AfterProbe");
        for offset in &mut host.char_offsets {
            *offset += 8;
        }
        host.char_count += 8;
        host.controls = vec![Control::Table(Box::new(fresh_table()))];
        host.line_segs = vec![line(0, 0, DECLARED, true), line(8, DECLARED, 1200, false)];
        assert_following_clears_host(&open(host), 1, "AfterProbe");
    }
    #[test]
    fn review_saved_host_before_and_after_grown_none_table() {
        let mut host = text("BeforeProbe\nAfterProbe");
        let anchor = "BeforeProbe\n".len() as u32;
        for offset in &mut host.char_offsets {
            if *offset >= anchor {
                *offset += 8;
            }
        }
        host.char_count += 8;
        host.controls = vec![Control::Table(Box::new(fresh_table()))];
        host.line_segs = vec![
            line(0, 0, 1200, false),
            line(anchor, 1200, DECLARED, true),
            line(anchor + 8, 1200 + DECLARED, 1200, false),
        ];
        assert_following_clears_host(&open(host), 1, "BeforeProbe\nAfterProbe");
    }
    #[test]
    fn review_two_grown_none_tables_on_separate_saved_lines() {
        let mut host = text("");
        host.char_count = 17;
        host.controls = vec![
            Control::Table(Box::new(fresh_table())),
            Control::Table(Box::new(fresh_table())),
        ];
        host.line_segs = vec![
            line(0, 0, DECLARED, true),
            line(8, DECLARED, DECLARED, true),
        ];
        assert_following_clears_host(&open(host), 2, "");
    }

    #[test]
    fn review_two_grown_none_tables_and_following_body_page_budget() {
        for body in [20000, 21000, 22000, 23000, 24000] {
            let mut host = text("");
            host.char_count = 17;
            host.controls = vec![
                Control::Table(Box::new(fresh_table())),
                Control::Table(Box::new(fresh_table())),
            ];
            host.line_segs = vec![
                line(0, 0, DECLARED, true),
                line(8, DECLARED, DECLARED, true),
            ];
            eprintln!("review two-table body budget={body}HU");
            assert_following_clears_host(&open_with_body_height(host, Some(body)), 2, "");
        }
    }
}
