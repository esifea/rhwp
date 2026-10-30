//! 관계형: 생성 TAC 줄의 공개 조판 결과에서 런 메트릭 출처를 보존한다.
//! 기존 source 단위 검사의 이관이며 픽셀 좌표나 시각 일치율을 고정하지 않는다.

use rhwp::model::paragraph::{CharShapeRef, LineSeg, Paragraph, SpaceMetric};
use rhwp::renderer::composer::compose_paragraph;

#[test]
fn generated_tac_rows_preserve_run_metric_provenance() {
    let mut table = rhwp::model::table::Table {
        page_break: rhwp::model::table::TablePageBreak::None,
        ..Default::default()
    };
    table.common.treat_as_char = true;
    table.common.flow_with_text = true;
    table.common.width = 1000;
    table.common.height = 1000;
    let para = Paragraph {
        text: "x\n".into(),
        char_count: 11,
        char_offsets: vec![8, 9],
        char_shapes: vec![CharShapeRef {
            start_pos: 0,
            char_shape_id: 0,
        }],
        controls: vec![rhwp::model::control::Control::Table(Box::new(table))],
        line_segs: vec![LineSeg {
            tag: LineSeg::TAG_SINGLE_SEGMENT_LINE | LineSeg::TAG_IMPLEMENTATION_PROPERTY,
            line_height: 1000,
            text_height: 1000,
            baseline_distance: 850,
            segment_width: 10000,
            ..Default::default()
        }],
        layout_space_metrics: vec![(0, SpaceMetric::HalfCell)],
        ..Default::default()
    };
    let composed = compose_paragraph(&para);
    assert_eq!(composed.lines.len(), 1);
    let line = &composed.lines[0];
    assert!(line.has_line_break);
    assert_eq!(line.runs.len(), 1);
    assert_eq!(line.runs[0].text, "x");
    assert!(line.runs[0].regenerated_font_metrics);
    assert_eq!(line.runs[0].space_metric, SpaceMetric::HalfCell);
}
