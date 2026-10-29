//! 독립 한컴 PDF의 글자 접두 공백과 표 사이 공백의 서로 다른 줄 소유를 검증한다.
#![cfg(not(target_arch = "wasm32"))]

use rhwp::model::{control::Control, table::Table};
use rhwp::renderer::font_environment::FontEnvironment;
use rhwp::renderer::render_tree::{RenderNode, RenderNodeType};
use rhwp::{parser, DocumentCore};

const DISTRIBUTION: &[u8] = include_bytes!("../../samples/task1768/distribution_doc.hwpx");
const SPACING: &[u8] = include_bytes!("../fixtures/table_spacing/table-spacing-cases.hwpx");
const ENVIRONMENT: &str = include_str!("../fixtures/table_spacing/environment.json");
const EPS: f64 = 0.001;

fn close(actual: f64, expected: f64, context: &str) {
    assert!(
        (actual - expected).abs() < EPS,
        "{context}: 실제 {actual}, 기대 {expected}"
    );
}

fn descendants(node: &RenderNode, predicate: fn(&RenderNodeType) -> bool) -> Vec<&RenderNode> {
    let mut found = Vec::new();
    if predicate(&node.node_type) {
        found.push(node);
    }
    for child in &node.children {
        found.extend(descendants(child, predicate));
    }
    found
}

fn column_nodes(root: &RenderNode) -> Vec<&RenderNode> {
    descendants(root, |kind| matches!(kind, RenderNodeType::Body { .. }))
        .into_iter()
        .flat_map(|body| descendants(body, |kind| matches!(kind, RenderNodeType::Column(_))))
        .flat_map(|column| &column.children)
        .collect()
}

fn text(node: &RenderNode) -> String {
    descendants(node, |kind| matches!(kind, RenderNodeType::TextRun(_)))
        .iter()
        .map(|node| match &node.node_type {
            RenderNodeType::TextRun(run) => run.text.as_str(),
            _ => unreachable!(),
        })
        .collect()
}

fn table(control: &Control) -> &Table {
    let Control::Table(table) = control else {
        panic!("원문 제어는 표여야 한다");
    };
    assert!(table.common.treat_as_char, "원문 표는 글줄 흐름에 참여한다");
    table
}

fn right(node: &RenderNode) -> f64 {
    node.bbox.x + node.bbox.width
}

fn bottom(node: &RenderNode) -> f64 {
    node.bbox.y + node.bbox.height
}

#[test]
fn original_text_prefix_advances_the_wrapped_table_on_its_physical_row() {
    let source = parser::parse_document(DISTRIBUTION).expect("변경 없는 배포용 HWP5 원문 파싱");
    let host = &source.sections[0].paragraphs[47];
    assert_eq!(
        host.text,
        format!("{}(단위 : 천원)   ", " ".repeat(82)),
        "원문 전체 글자와 공백"
    );
    assert_eq!(host.controls.len(), 1, "원문 host 표 제어 하나");
    let authored = table(&host.controls[0]);
    assert_eq!(
        host.control_text_positions(),
        vec![host.text.chars().count()],
        "마지막 원문 공백 다음의 표 제어"
    );
    let core = DocumentCore::from_bytes(DISTRIBUTION).expect("원본 글꼴 환경으로 배포 문서 열기");
    assert_eq!(core.page_count(), 3, "독립 PDF와 같은 전체 세 쪽");
    let pages: Vec<_> = (0..3)
        .map(|page| core.build_page_render_tree(page).expect("최종 쪽 트리"))
        .collect();
    assert!(
        core.build_page_render_tree(3).is_err(),
        "별도 빈 넷째 쪽 없음"
    );
    let mut lines = Vec::new();
    let mut objects = Vec::new();
    for (page, tree) in pages.iter().enumerate() {
        for node in column_nodes(&tree.root) {
            match &node.node_type {
                RenderNodeType::TextLine(owner)
                    if owner.section_index == Some(0) && owner.para_index == Some(47) =>
                {
                    lines.push((page, node))
                }
                RenderNodeType::Table(owner)
                    if owner.section_index == Some(0) && owner.para_index == Some(47) =>
                {
                    assert_eq!(owner.control_index, Some(0), "원문 표 제어 소유");
                    objects.push((page, node));
                }
                _ => {}
            }
        }
    }
    assert_eq!(objects.len(), 1, "원문 표는 세 쪽 전체에서 한 번만 배치");
    let (page, object) = objects[0];
    assert_eq!(page, 2, "독립 PDF의 셋째 쪽 표 소유");
    let rendered: String = lines.iter().map(|(_, line)| text(line)).collect();
    assert_eq!(
        rendered, host.text,
        "원문 host 전체 글자의 순서와 개수 보존"
    );
    assert_eq!(lines.len(), 2, "윗글 줄과 표의 물리 줄만 존재");
    let (upper_page, upper) = lines[0];
    let (row_page, row) = lines[1];
    assert_eq!(
        (upper_page, row_page),
        (page, page),
        "두 줄과 표의 같은 쪽 소유"
    );
    assert_eq!(
        text(upper),
        host.text
            .chars()
            .take(host.text.chars().count() - 1)
            .collect::<String>(),
        "독립 PDF의 윗줄은 말미 공백 둘을 보존"
    );
    assert_eq!(
        text(row),
        " ",
        "표의 물리 줄은 원문의 마지막 접두 공백 하나를 소유"
    );
    let runs = descendants(row, |kind| matches!(kind, RenderNodeType::TextRun(_)));
    assert_eq!(runs.len(), 1, "접두 공백을 소유하는 실제 텍스트 런 하나");
    let prefix = runs[0];
    let RenderNodeType::TextRun(owner) = &prefix.node_type else {
        unreachable!()
    };
    assert_eq!(
        owner.char_start,
        Some(host.text.chars().count() - 1),
        "원문 마지막 공백의 문자 소유"
    );
    assert_eq!(
        (owner.section_index, owner.para_index),
        (Some(0), Some(47)),
        "공백의 원문 문단 소유"
    );
    assert!(prefix.bbox.width > 0.0, "접두 공백은 양의 가로 전진을 보존");
    close(prefix.bbox.x, row.bbox.x, "표 줄 시작의 접두 공백");
    let left_margin = f64::from(authored.outer_margin_left) / 75.0;
    close(
        object.bbox.x,
        right(prefix) + left_margin,
        "표는 실제 접두 공백과 원문 왼쪽 바깥여백 뒤에 배치",
    );
    assert!(
        object.bbox.x - row.bbox.x > left_margin,
        "표 원점은 바깥여백 외에 접두 전진도 소비"
    );
    assert!(
        bottom(upper) <= row.bbox.y + EPS,
        "윗글과 표 줄의 세로 순서"
    );
    close(
        object.bbox.y - row.bbox.y,
        f64::from(authored.outer_margin_top) / 75.0,
        "원문 위 바깥여백",
    );
    close(
        bottom(row) - bottom(object),
        f64::from(authored.outer_margin_bottom) / 75.0,
        "표 전체를 소유하는 물리 줄 끝",
    );
    assert!(
        prefix.bbox.y >= row.bbox.y && bottom(prefix) <= bottom(row),
        "공백은 표 줄 안에 있고 별도 빈 줄을 만들지 않음"
    );
    assert!(
        right(object) + f64::from(authored.outer_margin_right) / 75.0 <= right(row) + EPS,
        "접두와 표의 전체 가로 점유"
    );
}

#[test]
fn object_separators_stay_with_the_first_table_when_the_second_wraps() {
    let source = parser::parse_document(SPACING).expect("변경 없는 여덟 구역 원문 파싱");
    assert_eq!(source.sections.len(), 8, "독립 PDF에 대응하는 여덟 구역");
    let mut core = DocumentCore::from_bytes(SPACING).expect("표 간격 원문 열기");
    core.set_font_environment(Some(
        FontEnvironment::from_json(ENVIRONMENT).expect("기존 호출자 글꼴 환경"),
    ))
    .expect("검증한 HCR 환경 선택");
    assert_eq!(core.page_count(), 8, "독립 PDF와 같은 전체 여덟 쪽");
    assert!(
        core.build_page_render_tree(8).is_err(),
        "별도 빈 아홉째 쪽 없음"
    );
    for (index, section) in source.sections.iter().enumerate() {
        assert_eq!(section.paragraphs.len(), 3, "앞글·표 host·뒷글의 원문 순서");
        let host = &section.paragraphs[1];
        assert_eq!(host.text, "  ", "변경 없는 원문 표 사이 공백 둘");
        assert_eq!(host.controls.len(), 2, "연속한 원문 표 제어 둘");
        assert_eq!(
            host.control_text_positions(),
            vec![0, 2],
            "첫 표·공백 둘·둘째 표의 원문 순서"
        );
        let authored = [table(&host.controls[0]), table(&host.controls[1])];
        let tree = core
            .build_page_render_tree(index as u32)
            .expect("최종 쪽 트리");
        let nodes = column_nodes(&tree.root);
        let objects: Vec<_> = nodes
            .iter()
            .copied()
            .filter(|node| matches!(node.node_type, RenderNodeType::Table(_)))
            .collect();
        assert_eq!(objects.len(), 2, "각 쪽의 원문 표는 한 번씩 배치");
        for (control, object) in objects.iter().enumerate() {
            let RenderNodeType::Table(owner) = &object.node_type else {
                unreachable!()
            };
            assert_eq!(
                (owner.section_index, owner.para_index, owner.control_index),
                (Some(index), Some(1), Some(control)),
                "정확한 구역·문단·표 제어 소유와 순서"
            );
            let expected: String = authored[control]
                .cells
                .iter()
                .flat_map(|cell| &cell.paragraphs)
                .map(|para| para.text.as_str())
                .collect();
            assert_eq!(
                text(object),
                expected,
                "표별 원문 전체 내용의 순서와 한 번 소유"
            );
        }
        let lines: Vec<_> = nodes.iter().copied().filter(|node| matches!(&node.node_type, RenderNodeType::TextLine(owner) if owner.para_index == Some(1))).collect();
        assert_eq!(
            lines.len(),
            1,
            "표 사이 공백은 첫 표의 물리 줄 하나에만 소속"
        );
        let row = lines[0];
        assert_eq!(text(row), host.text, "원문 공백 둘의 개수와 순서 보존");
        let runs = descendants(row, |kind| matches!(kind, RenderNodeType::TextRun(_)));
        assert_eq!(runs.len(), 1, "표 사이 공백의 런 하나");
        let separator = runs[0];
        let RenderNodeType::TextRun(owner) = &separator.node_type else {
            unreachable!()
        };
        assert_eq!(
            (owner.section_index, owner.para_index, owner.char_start),
            (Some(index), Some(1), Some(0)),
            "원문 두 공백의 정확한 소유"
        );
        assert!(separator.bbox.width > 0.0, "표 사이 원문 공백의 실제 전진");
        close(
            separator.bbox.x,
            right(objects[0]) + f64::from(authored[0].outer_margin_right) / 75.0,
            "공백은 첫 표 전체 점유 다음에 남음",
        );
        assert!(
            row.bbox.y <= objects[0].bbox.y && bottom(row) >= bottom(objects[0]),
            "첫 표와 공백의 같은 물리 줄 소유"
        );
        let before = nodes.iter().find(|node| matches!(&node.node_type, RenderNodeType::TextLine(owner) if owner.para_index == Some(0))).expect("앞 본문 줄");
        close(
            objects[0].bbox.x,
            before.bbox.x + f64::from(authored[0].outer_margin_left) / 75.0,
            "첫 표의 본문 원점과 원문 여백",
        );
        let expected = format!(
            "{}{}{}{}{}",
            section.paragraphs[0].text,
            host.text,
            text(objects[0]),
            text(objects[1]),
            section.paragraphs[2].text
        );
        assert_eq!(
            text(&tree.root),
            expected,
            "각 쪽 전체 원문 내용의 순서·누락·중복 검사"
        );
        if index < 7 {
            close(
                objects[1].bbox.x,
                before.bbox.x + f64::from(authored[1].outer_margin_left) / 75.0,
                "줄바꿈한 둘째 표는 앞 표의 공백 전진 없이 본문 왼쪽에서 시작",
            );
            assert!(
                objects[1].bbox.y >= bottom(row) - EPS,
                "둘째 표는 첫 표의 물리 줄 뒤에 소속"
            );
        } else {
            // 독립 PDF의 여덟째 쪽은 두 표와 공백을 같은 물리 줄에 둔다.
            close(
                objects[1].bbox.x,
                right(separator) + f64::from(authored[1].outer_margin_left) / 75.0,
                "같은 줄의 둘째 표는 실제 공백 전진을 소비",
            );
            assert!(
                objects[1].bbox.y < bottom(objects[0]),
                "같은 줄 대조군을 강제로 줄바꿈하지 않음"
            );
            assert!(
                row.bbox.y <= objects[1].bbox.y && bottom(row) >= bottom(objects[1]),
                "두 표의 전체 세로 점유"
            );
            close(
                right(row),
                right(objects[1]) + f64::from(authored[1].outer_margin_right) / 75.0,
                "같은 줄의 두 표와 공백 전체 가로 점유",
            );
        }
    }
}
