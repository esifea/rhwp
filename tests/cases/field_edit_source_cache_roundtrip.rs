//! Native field edits preserve rendered geometry and source line caches through save/reload.

use std::fs;
use std::path::Path;

use rhwp::document_core::queries::field_query::{FieldInfo, FieldLocation, NestedEntry};
use rhwp::document_core::DocumentCore;
use rhwp::model::control::{Control, FieldType};
use rhwp::model::paragraph::Paragraph;

fn load_sample(relative: &str) -> DocumentCore {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(relative);
    let bytes = fs::read(&path).unwrap_or_else(|e| panic!("read {}: {}", path.display(), e));
    DocumentCore::from_bytes(&bytes).unwrap_or_else(|e| panic!("parse {}: {e:?}", path.display()))
}

fn field_named<'a>(core: &'a DocumentCore, name: &str) -> FieldInfo {
    core.collect_all_fields().into_iter()
        .find(|field| field.field.field_name() == Some(name))
        .unwrap_or_else(|| panic!("field {name:?} should exist"))
}

fn paragraph_at_location<'a>(core: &'a DocumentCore, location: &FieldLocation) -> &'a Paragraph {
    let mut paragraph =
        &core.document().sections[location.section_index].paragraphs[location.para_index];
    for entry in &location.nested_path {
        paragraph = match entry {
            NestedEntry::TableCell {
                control_index,
                cell_index,
                para_index,
            } => {
                let Control::Table(table) = &paragraph.controls[*control_index] else {
                    panic!("field path should point at a table")
                };
                &table.cells[*cell_index].paragraphs[*para_index]
            }
            NestedEntry::TextBox {
                control_index,
                para_index,
            } => {
                let Control::Shape(shape) = &paragraph.controls[*control_index] else {
                    panic!("field path should point at a shape textbox")
                };
                &shape
                    .drawing()
                    .and_then(|drawing| drawing.text_box.as_ref())
                    .expect("field shape should own a textbox")
                    .paragraphs[*para_index]
            }
        };
    }
    paragraph
}

fn char_shape_signature(paragraph: &Paragraph) -> Vec<(u32, u32)> {
    paragraph
        .char_shapes
        .iter()
        .map(|shape| (shape.start_pos, shape.char_shape_id))
        .collect()
}

fn line_position_signature(paragraph: &Paragraph) -> Vec<(u32, i32)> {
    paragraph
        .line_segs
        .iter()
        .map(|line| (line.text_start, line.vertical_pos))
        .collect()
}

fn field_range_signature(paragraph: &Paragraph) -> Vec<(usize, usize, usize)> {
    paragraph
        .field_ranges
        .iter()
        .map(|range| (range.start_char_idx, range.end_char_idx, range.control_idx))
        .collect()
}

fn rendered_field_geometry(
    core: &DocumentCore,
    value: &str,
    phase: &str,
) -> Vec<(u32, serde_json::Value)> {
    assert!(!value.is_empty(), "rendered field value must be nonempty");
    let mut matches = Vec::new();
    for page in 0..core.page_count() {
        let layout: serde_json::Value = serde_json::from_str(
            &core
                .get_page_text_layout_native(page)
                .expect("field page layout"),
        ).expect("parse field page layout");
        let runs: Vec<_> = layout["runs"]
            .as_array()
            .expect("text runs")
            .iter()
            .filter(|run| {
                run["text"].as_str().is_some_and(|text| text.contains(value))
            }).collect();
        if runs.is_empty() {
            continue;
        }
        let svg = core.render_page_svg_native(page).expect("render field page");
        let painted = roxmltree::Document::parse(&svg).expect("parse rendered field SVG");
        let visible: String = painted.descendants().filter(|node| {
                node.is_text()
                    && node
                        .ancestors()
                        .any(|parent| parent.has_tag_name("text") || parent.has_tag_name("tspan"))
            }).filter_map(|node| node.text()).collect();
        // SVG omits space/tab clusters; exact run text and charX still check spacing.
        let visible: String = visible.chars().filter(|ch| !ch.is_whitespace()).collect();
        let glyphs: String = value.chars().filter(|ch| !ch.is_whitespace()).collect();
        assert!(
            visible.contains(&glyphs),
            "{phase}: edited glyphs {glyphs:?} missing from page {page} SVG: {visible:?}"
        );
        for run in runs {
            for key in ["x", "y", "w", "h"] {
                assert!(
                    run[key].as_f64().is_some_and(f64::is_finite),
                    "finite field {key}"
                );
            }
            assert!(
                run["w"].as_f64().unwrap() > 0.0 && run["h"].as_f64().unwrap() > 0.0,
                "edited field must occupy visible space"
            );
            matches.push((
                page,
                serde_json::json!({
                    "text": run["text"], "x": run["x"], "y": run["y"],
                    "w": run["w"], "h": run["h"], "charX": run["charX"],
                }),
            ));
        }
    }
    assert!(
        !matches.is_empty(),
        "{phase}: edited value {value:?} missing from rendered text runs"
    );
    matches
}

fn assert_field_render_roundtrip(before: &DocumentCore, after: &DocumentCore, name: &str) {
    let field = field_named(before, name);
    let before_rows = paragraph_at_location(before, &field.location);
    let loaded = field_named(after, name);
    let after_rows = paragraph_at_location(after, &loaded.location);
    eprintln!(
        "field {name:?}: live={:?}, source={:?}, reload={:?}",
        line_position_signature(before_rows),
        before_rows.serializable_line_segs(),
        line_position_signature(after_rows)
    );
    assert_eq!(
        after.page_count(),
        before.page_count(),
        "field save/reload page count"
    );
    let expected = rendered_field_geometry(before, &field.value, "before native save");
    let actual = rendered_field_geometry(after, &field.value, "after native reload");
    assert_eq!(
        actual, expected,
        "visible edited field page/geometry after native save/reload"
    );
}

fn record_original_field_cache(core: &DocumentCore, field: &FieldInfo) {
    let para = paragraph_at_location(core, &field.location);
    eprintln!(
        "original field {:?}: live={:?}, source={:?}",
        field.field.field_name(),
        line_position_signature(para),
        para.serializable_line_segs()
    );
}

fn assert_field_layout_roundtrip(
    before_save: &DocumentCore,
    after_load: &DocumentCore,
    name: &str,
) {
    let before_field = field_named(before_save, name);
    let after_field = field_named(after_load, name);
    assert_eq!(
        after_field.value, before_field.value,
        "field value roundtrip"
    );

    let before = paragraph_at_location(before_save, &before_field.location);
    let after = paragraph_at_location(after_load, &after_field.location);
    assert!(
        !before.line_segs.is_empty(),
        "field edit should materialize owner LineSeg before serialization"
    );
    assert_eq!(
        line_position_signature(after),
        line_position_signature(before),
        "persisted field owner LineSeg count/start/vpos should survive roundtrip"
    );
    assert_eq!(
        char_shape_signature(after),
        char_shape_signature(before),
        "field owner char-shape boundaries should survive roundtrip"
    );
    assert_eq!(
        after.char_count, before.char_count,
        "field owner char_count"
    );
    assert_eq!(
        after.char_offsets, before.char_offsets,
        "field owner char_offsets should survive roundtrip"
    );
    assert_eq!(
        field_range_signature(after),
        field_range_signature(before),
        "field ranges should survive roundtrip"
    );
}

#[test]
fn two_empty_native_fields_keep_rendered_geometry_after_save_reload() {
    let mut core = load_sample("samples/field-01.hwp");
    let fields = core.collect_all_fields();
    let empty_fields: Vec<_> = fields.iter()
        .filter(|f| f.field.field_type == FieldType::ClickHere && f.value.is_empty())
        .collect();
    assert!(
        empty_fields.len() >= 2,
        "field-01.hwp should contain at least two empty ClickHere fields"
    );

    let id1 = empty_fields[0].field.field_id;
    let id2 = empty_fields[1].field.field_id;
    record_original_field_cache(&core, empty_fields[0]);
    record_original_field_cache(&core, empty_fields[1]);
    core.set_field_value_by_id(id1, "테스트회사").expect("set first field");
    core.set_field_value_by_id(id2, "테스트작성자").expect("set second field");

    let saved = core.export_hwp_native().expect("export hwp");
    let reparsed = DocumentCore::from_bytes(&saved).expect("reparse exported hwp");
    assert_field_render_roundtrip(&core, &reparsed, "회사명");
    assert_field_render_roundtrip(&core, &reparsed, "작성자");
}

#[test]
fn nested_native_table_field_keeps_rendered_geometry_after_save_reload() {
    const SAMPLE: &str = "samples/76076_regulatory_analysis.hwp";
    const NAME: &str = "안건명";
    let mut core = load_sample(SAMPLE);
    let field = field_named(&core, NAME);
    assert_ne!(
        field.field.ctrl_id, 0,
        "fixture must use a real ClickHere control"
    );
    assert_eq!(field.location.nested_path.len(), 1);
    record_original_field_cache(&core, &field);

    core.set_field_value_by_id(field.field.field_id, "검증 안건명").expect("set nested HWP ClickHere by id");
    let saved = core.export_hwp_native().expect("export edited HWP");
    let reparsed = DocumentCore::from_bytes(&saved).expect("reparse edited HWP");
    assert_field_render_roundtrip(&core, &reparsed, NAME);
}

#[test]
fn native_textbox_field_keeps_rendered_geometry_after_save_reload() {
    const SAMPLE: &str = "samples/basic/BlogForm_BookReview.hwp";
    const NAME: &str = "이곳에 책 표지 그림을 넣으세요.";
    let mut core = load_sample(SAMPLE);
    let field = field_named(&core, NAME);
    assert!(matches!(
        field.location.nested_path.as_slice(),
        [NestedEntry::TableCell { .. }, NestedEntry::TextBox { .. }]
    ));
    record_original_field_cache(&core, &field);

    core.set_field_value_by_name(NAME, "검증표지").expect("set textbox ClickHere by name");
    let saved = core.export_hwp_native().expect("export edited HWP");
    let reparsed = DocumentCore::from_bytes(&saved).expect("reparse edited HWP");
    assert_field_render_roundtrip(&core, &reparsed, NAME);
}

#[test]
fn repeated_native_field_clear_and_reflow_keep_source_backed_geometry() {
    let mut core = load_sample("samples/field-01.hwp");
    let name = "회사명";
    let original = field_named(&core, name);
    assert!(
        !paragraph_at_location(&core, &original.location).serializable_line_segs().is_empty(),
        "control requires genuinely source-backed native owner rows"
    );
    for value in ["첫검증회사", "둘검증회사"] {
        core.set_field_value_by_id(original.field.field_id, value).expect("repeat native field edit");
        let edited = field_named(&core, name);
        let owner = paragraph_at_location(&core, &edited.location);
        assert!(
            !owner.line_segs.is_empty(),
            "native owner reflow must materialize rows"
        );
        assert_eq!(
            owner.serializable_line_segs(),
            owner.line_segs.as_slice(),
            "clear/regenerate must retain source-backed owner identity"
        );
        let source_rows = owner.serializable_line_segs().to_vec();
        let saved = core.export_hwp_native().expect("save repeated native field edit");
        let loaded = DocumentCore::from_bytes(&saved).expect("reload repeated native field edit");
        let actual = field_named(&loaded, name);
        assert_eq!(
            paragraph_at_location(&loaded, &actual.location).line_segs,
            source_rows,
            "all genuine native row metrics must persist"
        );
        assert_field_render_roundtrip(&core, &loaded, name);
        assert_field_layout_roundtrip(&core, &loaded, name);
        core = loaded;
    }
}
