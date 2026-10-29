//! 독립 한컴 PDF의 분리된 앵커 쪽과 완전한 TABLE 쪽 소유를 검증한다.
#![cfg(not(target_arch = "wasm32"))]

use rhwp::model::{control::Control, table::Table};
use rhwp::renderer::font_environment::FontEnvironment;
use rhwp::renderer::render_tree::{RenderNode, RenderNodeType};
use rhwp::{parser, DocumentCore};

const SOURCE: &[u8] =
    include_bytes!("../fixtures/generated_atomic/atomic-row-cases-without-line-cache.hwpx");
const ENVIRONMENT: &str = include_str!("../fixtures/generated_atomic/environment.json");
const EPS: f64 = 0.001;

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

fn text(node: &RenderNode) -> String {
    descendants(node, |kind| matches!(kind, RenderNodeType::TextRun(_)))
        .iter()
        .map(|node| match &node.node_type {
            RenderNodeType::TextRun(run) => run.text.as_str(),
            _ => unreachable!(),
        })
        .collect()
}

fn words(value: &str) -> Vec<&str> {
    value.split_whitespace().collect()
}

fn tables(node: &RenderNode) -> Vec<&RenderNode> {
    descendants(node, |kind| matches!(kind, RenderNodeType::Table(_)))
}

fn only_table(page: &RenderNode) -> &RenderNode {
    let found = tables(page);
    assert_eq!(found.len(), 1, "표 쪽에는 완전한 표 하나만 존재");
    found[0]
}

fn body(page: &RenderNode) -> &RenderNode {
    let found = descendants(page, |kind| matches!(kind, RenderNodeType::Body { .. }));
    assert_eq!(found.len(), 1, "각 쪽 본문 하나");
    found[0]
}

fn bottom(node: &RenderNode) -> f64 {
    node.bbox.y + node.bbox.height
}

fn right(node: &RenderNode) -> f64 {
    node.bbox.x + node.bbox.width
}

fn close(actual: f64, expected: f64, context: &str) {
    assert!(
        (actual - expected).abs() < EPS,
        "{context}: 실제 {actual}, 기대 {expected}"
    );
}

fn authored_table(controls: &[Control]) -> (usize, &Table) {
    let found: Vec<_> = controls
        .iter()
        .enumerate()
        .filter_map(|(index, control)| match control {
            Control::Table(table) => Some((index, table.as_ref())),
            _ => None,
        })
        .collect();
    assert_eq!(found.len(), 1, "원문 host가 소유하는 표 하나");
    found[0]
}

fn pages() -> Vec<RenderNode> {
    let mut core = DocumentCore::from_bytes(SOURCE).expect("변경 없는 독립 기준 원문 열기");
    core.set_font_environment(Some(
        FontEnvironment::from_json(ENVIRONMENT).expect("검증한 호출자 HCR 환경"),
    ))
    .expect("호출자 환경 선택");
    let pages = (0..core.page_count())
        .map(|page| {
            core.build_page_render_tree(page)
                .expect("실제 최종 쪽 트리")
                .root
        })
        .collect();
    assert!(
        core.build_page_render_tree(core.page_count()).is_err(),
        "마지막 내용 뒤의 별도 빈 쪽 없음"
    );
    pages
}

#[test]
fn five_sections_keep_following_and_host_on_their_anchor_pages() {
    let pages = pages();
    assert_eq!(
        pages.len(),
        10,
        "독립 PDF의 구역별 앵커·표 두 쪽, 전체 열 쪽"
    );
    for section in 0..5 {
        let anchor = &pages[section * 2];
        let object_page = &pages[section * 2 + 1];
        assert!(
            tables(anchor).is_empty(),
            "앵커 쪽에는 표 조각이 없어야 한다"
        );
        let expected = if section == 2 {
            vec!["HostText", "Following", "body", "marker"]
        } else {
            vec!["Following", "body", "marker"]
        };
        assert_eq!(
            words(&text(anchor)),
            expected,
            "앵커의 정확한 본문 순서와 한 번 소유"
        );
        let table = only_table(object_page);
        assert_eq!(text(object_page), text(table), "표 쪽은 표 내용만 소유");
        let RenderNodeType::Table(owner) = &table.node_type else {
            unreachable!()
        };
        assert_eq!(
            owner.section_index,
            Some(section),
            "다음 물리 쪽 표의 원문 구역 소유"
        );
        let following = descendants(anchor, |kind| matches!(kind, RenderNodeType::TextRun(_)));
        assert_eq!(
            following.len(),
            2,
            "빈 host도 자기 줄과 뒤 본문 줄을 한 번씩 소유"
        );
        for (para, run) in following.into_iter().enumerate() {
            let RenderNodeType::TextRun(owner) = &run.node_type else {
                unreachable!()
            };
            assert_eq!(
                owner.section_index,
                Some(section),
                "앵커 글자의 원문 구역 소유"
            );
            assert_eq!(
                owner.para_index,
                Some(para),
                "host와 뒤 본문의 서로 다른 원문 문단 소유"
            );
            assert_eq!(
                owner.text,
                if para == 1 {
                    "Following body marker"
                } else if section == 2 {
                    "HostText "
                } else {
                    ""
                },
                "빈 host와 보이는 host 모두 원문 글자 그대로 보존"
            );
            assert!(
                run.bbox.y >= body(anchor).bbox.y - EPS
                    && bottom(run) <= bottom(body(anchor)) + EPS,
                "뒤 본문과 host 글자는 자신의 앵커 본문 안에 표시"
            );
        }
    }
}

#[test]
fn complete_tables_preserve_every_source_cell_and_connected_rowspan() {
    let source = parser::parse_document(SOURCE).expect("변경 없는 다섯 구역 원문 파싱");
    assert_eq!(source.sections.len(), 5, "원문의 다섯 독립 사례");
    let pages = pages();
    let all_segments: Vec<_> = (1..=200)
        .map(|index| format!("segment{index:03}"))
        .collect();
    for (section, authored) in source.sections.iter().enumerate() {
        let (control, original) = authored_table(&authored.paragraphs[0].controls);
        let objects: Vec<_> = pages.iter().flat_map(tables).filter(|node| {
            matches!(&node.node_type, RenderNodeType::Table(owner) if owner.section_index == Some(section))
        }).collect();
        assert_eq!(objects.len(), 1, "각 구역 표는 전체 문서에서 한 번만 배치");
        let table = objects[0];
        let RenderNodeType::Table(owner) = &table.node_type else {
            unreachable!()
        };
        assert_eq!(
            (
                owner.para_index,
                owner.control_index,
                owner.row_count,
                owner.col_count
            ),
            (
                Some(0),
                Some(control),
                original.row_count,
                original.col_count
            ),
            "원문 host·제어와 완전한 행·열 소유"
        );
        let cells = descendants(table, |kind| matches!(kind, RenderNodeType::TableCell(_)));
        assert_eq!(
            cells.len(),
            original.cells.len(),
            "원문 셀을 나누거나 복제하지 않음"
        );
        for (cell, original_cell) in cells.iter().zip(&original.cells) {
            let RenderNodeType::TableCell(owner) = &cell.node_type else {
                unreachable!()
            };
            assert_eq!(
                (owner.row, owner.col, owner.row_span, owner.col_span),
                (
                    original_cell.row,
                    original_cell.col,
                    original_cell.row_span,
                    original_cell.col_span
                ),
                "원문 셀 주소와 병합 범위 보존"
            );
            assert!(
                !owner.page_fragment,
                "완전한 TABLE 셀은 쪽 분할 조각이 아님"
            );
            let expected: String = original_cell
                .paragraphs
                .iter()
                .map(|para| para.text.as_str())
                .collect();
            assert_eq!(
                words(&text(cell)),
                words(&expected),
                "각 셀 전체 원문의 순서·누락·중복 검사"
            );
            assert!(
                cell.bbox.x >= table.bbox.x - EPS
                    && right(cell) <= right(table) + EPS
                    && cell.bbox.y >= table.bbox.y - EPS
                    && bottom(cell) <= bottom(table) + EPS,
                "셀은 자신이 소유한 완전한 표 상자 안에 배치"
            );
            for line in descendants(cell, |kind| matches!(kind, RenderNodeType::TextLine(_))) {
                assert!(
                    line.bbox.y >= cell.bbox.y - EPS && bottom(line) <= bottom(cell) + EPS,
                    "긴 줄의 가로 잉크 넘침과 무관하게 글줄은 소유 셀의 세로 범위 안에 배치"
                );
            }
        }
        let rendered = cells
            .iter()
            .map(|cell| text(cell))
            .collect::<Vec<_>>()
            .join(" ");
        let segments: Vec<_> = words(&rendered)
            .into_iter()
            .filter(|token| token.starts_with("segment"))
            .collect();
        assert_eq!(
            segments, all_segments,
            "200개 토큰을 원문 순서로 정확히 한 번 보존"
        );
        if section == 1 {
            let first = cells[0];
            let spanning = cells[1];
            let second = cells[2];
            assert_eq!(text(first), "Label0", "첫 왼쪽 셀의 표지");
            assert_eq!(text(second), "Label1", "둘째 왼쪽 셀의 표지");
            close(
                second.bbox.y - first.bbox.y,
                f64::from(original.cells[0].height) / 75.0,
                "원문 첫 행 높이 뒤의 둘째 행 원점",
            );
            close(
                spanning.bbox.y,
                first.bbox.y,
                "연결된 오른쪽 병합 셀의 시작 행",
            );
            close(
                bottom(spanning),
                bottom(second),
                "병합 셀은 마지막 연결 행까지 소유",
            );
        }
    }
}

#[test]
fn offset_and_hold_share_the_fresh_origin_without_truncating_the_atomic_extent() {
    let source = parser::parse_document(SOURCE).expect("원문 위치와 바깥여백 파싱");
    let pages = pages();
    assert_eq!(pages.len(), 10, "독립 PDF의 전체 쪽 소유");
    let reference = only_table(&pages[1]);
    assert_eq!(
        authored_table(&source.sections[3].paragraphs[0].controls)
            .1
            .common
            .vertical_offset,
        300,
        "오프셋 대조군의 원문 300HU 보존"
    );
    for section in 0..5 {
        let page = &pages[section * 2 + 1];
        let table = only_table(page);
        let (_, original) = authored_table(&source.sections[section].paragraphs[0].controls);
        close(
            table.bbox.y - body(page).bbox.y,
            f64::from(original.outer_margin_top) / 75.0,
            "독립 PDF의 새 본문 원점에는 원문 위 바깥여백만 한 번 적용",
        );
        close(
            table.bbox.x - body(page).bbox.x,
            f64::from(original.outer_margin_left) / 75.0,
            "새 쪽 본문 왼쪽 원점과 원문 바깥여백",
        );
        close(
            table.bbox.y,
            reference.bbox.y,
            "offset·hold·host·rowspan에도 같은 새 쪽 원점",
        );
        assert!(
            bottom(table) > bottom(page),
            "나누지 않는 TABLE의 전체 높이는 실제 종이 끝을 넘음"
        );
        let visible: Vec<_> = descendants(table, |kind| matches!(kind, RenderNodeType::TextRun(_)))
            .into_iter()
            .filter(|node| {
                let RenderNodeType::TextRun(run) = &node.node_type else {
                    unreachable!()
                };
                node.bbox.y + run.baseline <= bottom(page)
            })
            .flat_map(|node| {
                let RenderNodeType::TextRun(run) = &node.node_type else {
                    unreachable!()
                };
                words(&run.text)
                    .into_iter()
                    .filter(|token| token.starts_with("segment"))
            })
            .collect();
        let expected: Vec<_> = (1..=36).map(|index| format!("segment{index:03}")).collect();
        assert_eq!(
            visible, expected,
            "독립 PDF에서 종이 안에 기준선이 있는 토큰은 001..036뿐"
        );
    }
}
