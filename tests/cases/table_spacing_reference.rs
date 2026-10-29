//! 독립 한컴 PDF와 원문 HU로 표 간격 및 최종 줄 소유를 검증한다.
#![cfg(not(target_arch = "wasm32"))]

use rhwp::renderer::font_environment::FontEnvironment;
use rhwp::renderer::render_tree::{RenderNode, RenderNodeType};
use rhwp::DocumentCore;

const SOURCE: &[u8] = include_bytes!("../fixtures/table_spacing/table-spacing-cases.hwpx");
const ENVIRONMENT: &str = include_str!("../fixtures/table_spacing/environment.json");
const EPS: f64 = 0.001;

fn px(hu: u32) -> f64 {
    f64::from(hu) / 75.0
}

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

fn text(node: &RenderNode) -> String {
    descendants(node, |kind| matches!(kind, RenderNodeType::TextRun(_)))
        .iter()
        .map(|node| match &node.node_type {
            RenderNodeType::TextRun(run) => run.text.as_str(),
            _ => unreachable!(),
        })
        .collect()
}

fn tables(page: &RenderNode) -> Vec<&RenderNode> {
    descendants(page, |kind| matches!(kind, RenderNodeType::Table(_)))
}

fn cells(table: &RenderNode) -> Vec<&RenderNode> {
    descendants(table, |kind| matches!(kind, RenderNodeType::TableCell(_)))
}

fn unique<'a>(nodes: Vec<&'a RenderNode>, marker: &str) -> &'a RenderNode {
    let found: Vec<_> = nodes
        .into_iter()
        .filter(|node| text(node) == marker)
        .collect();
    assert_eq!(found.len(), 1, "{marker} 소유 노드가 정확히 하나여야 한다");
    found[0]
}

fn run<'a>(page: &'a RenderNode, marker: &str) -> &'a RenderNode {
    unique(
        descendants(page, |kind| matches!(kind, RenderNodeType::TextRun(_))),
        marker,
    )
}

fn host(page: &RenderNode) -> &RenderNode {
    let found = descendants(
        page,
        |kind| matches!(kind, RenderNodeType::TextLine(line) if line.para_index == Some(1)),
    );
    assert_eq!(found.len(), 1, "두 공백을 소유하는 host 글줄 하나");
    found[0]
}

fn bottom(node: &RenderNode) -> f64 {
    node.bbox.y + node.bbox.height
}

fn right(node: &RenderNode) -> f64 {
    node.bbox.x + node.bbox.width
}

fn contained(outer: &RenderNode, inner: &RenderNode) {
    assert!(
        inner.bbox.x >= outer.bbox.x - EPS
            && inner.bbox.y >= outer.bbox.y - EPS
            && right(inner) <= right(outer) + EPS
            && bottom(inner) <= bottom(outer) + EPS,
        "안쪽 내용 {:?}는 소유 상자 {:?} 안에 있어야 한다",
        inner.bbox,
        outer.bbox
    );
}

fn pages() -> Vec<RenderNode> {
    let mut core = DocumentCore::from_bytes(SOURCE).expect("변경 없는 독립 기준 원문 열기");
    core.set_font_environment(Some(
        FontEnvironment::from_json(ENVIRONMENT).expect("검증한 호출자 HCR 환경"),
    ))
    .expect("호출자 환경 선택");
    assert_eq!(core.page_count(), 8, "독립 PDF와 같은 전체 8쪽");
    let pages = (0..8)
        .map(|page| {
            core.build_page_render_tree(page)
                .expect("최종 쪽 트리")
                .root
        })
        .collect();
    assert!(core.build_page_render_tree(8).is_err(), "빈 아홉째 쪽 없음");
    pages
}

#[test]
fn all_eight_pages_keep_content_owners_and_following_body_clear() {
    for (index, page) in pages().iter().enumerate() {
        let mut expected = vec![
            "BeforeMarker",
            "FirstCell",
            "DerivedWidthMarker",
            "SecondCell",
            "FollowingMarker",
        ];
        if index == 4 {
            expected.extend(["Row2Left", "Row2Right"]);
        } else if index == 5 {
            expected.push("Row2MergedCell");
        }
        let mut actual: Vec<_> =
            descendants(page, |kind| matches!(kind, RenderNodeType::TextRun(_)))
                .iter()
                .map(|node| text(node))
                .filter(|value| !value.trim().is_empty())
                .collect();
        actual.sort();
        expected.sort();
        assert_eq!(actual, expected, "각 쪽의 정확한 내용과 한 번 소유");
        assert_eq!(text(host(page)), "  ", "원문의 두 공백도 실제 줄 내용");
        let objects = tables(page);
        assert_eq!(objects.len(), 2, "원문 표 두 개를 한 번씩 배치");
        let expected_cells: &[(u16, u16, &str)] = match index {
            4 => &[
                (0, 0, "FirstCell"),
                (0, 1, "DerivedWidthMarker"),
                (1, 0, "Row2Left"),
                (1, 1, "Row2Right"),
            ],
            5 => &[
                (0, 0, "FirstCell"),
                (0, 1, "DerivedWidthMarker"),
                (1, 0, "Row2MergedCell"),
            ],
            _ => &[(0, 0, "FirstCell"), (0, 1, "DerivedWidthMarker")],
        };
        let actual_cells: Vec<_> = cells(objects[0])
            .iter()
            .map(|cell| {
                let RenderNodeType::TableCell(owner) = &cell.node_type else {
                    unreachable!()
                };
                (owner.row, owner.col, text(cell))
            })
            .collect();
        let expected_cells: Vec<_> = expected_cells
            .iter()
            .map(|(row, col, label)| (*row, *col, (*label).to_owned()))
            .collect();
        assert_eq!(
            actual_cells, expected_cells,
            "각 쪽 첫 표의 정확한 셀·내용 소유와 순서"
        );
        assert_eq!(text(objects[1]), "SecondCell", "둘째 표만 둘째 내용을 소유");

        let body = descendants(page, |kind| matches!(kind, RenderNodeType::Body { .. }));
        assert_eq!(body.len(), 1, "쪽 본문 하나");
        for (control, table) in objects.iter().enumerate() {
            let RenderNodeType::Table(owner) = &table.node_type else {
                unreachable!()
            };
            assert_eq!(owner.section_index, Some(index), "원문 구역 소유");
            assert_eq!(owner.para_index, Some(1), "원문 host 문단 소유");
            assert_eq!(
                owner.control_index,
                Some(control),
                "원문 표 순서와 제어 소유"
            );
            contained(body[0], table);
            for cell in cells(table) {
                contained(table, cell);
                for line in descendants(cell, |kind| matches!(kind, RenderNodeType::TextLine(_))) {
                    contained(cell, line);
                }
            }
            assert!(
                bottom(run(page, "BeforeMarker")) <= table.bbox.y + EPS,
                "앞 본문과 표가 겹치지 않아야 한다"
            );
            assert!(
                run(page, "FollowingMarker").bbox.y >= bottom(table) - EPS,
                "뒤 본문이 두 표 외곽을 모두 비워야 한다"
            );
        }
        assert!(
            !objects[0].bbox.intersects(&objects[1].bbox),
            "두 표가 겹치면 안 된다"
        );
        contained(body[0], run(page, "FollowingMarker"));
        if index < 7 {
            assert!(
                bottom(objects[0]) <= objects[1].bbox.y + EPS,
                "다른 줄의 표는 세로 순서를 보존"
            );
        }
    }
}

#[test]
fn positive_spacing_insets_cells_and_advances_the_complete_frame() {
    let pages = pages();
    let zero = tables(&pages[0])[0];
    let positive = tables(&pages[1])[0];
    close(
        positive.bbox.width,
        px(20900),
        "두 10000 열과 내부 1개·외곽 2개 300HU 밴드",
    );
    close(
        positive.bbox.height,
        px(7800),
        "7200 행과 위아래 300HU 밴드",
    );
    close(
        positive.bbox.width - zero.bbox.width,
        px(900),
        "간격0 대조군 대비 세 가로 밴드",
    );
    close(
        positive.bbox.height - zero.bbox.height,
        px(600),
        "간격0 대비 두 세로 밴드",
    );
    for marker in ["FirstCell", "DerivedWidthMarker"] {
        close(
            run(&pages[1], marker).bbox.y - run(&pages[0], marker).bbox.y,
            px(300),
            "위 외곽 밴드가 셀 원점을 한 번 이동",
        );
    }
    close(
        run(&pages[1], "FirstCell").bbox.x - run(&pages[0], "FirstCell").bbox.x,
        px(300),
        "왼쪽 외곽 밴드",
    );
    for marker in ["SecondCell", "FollowingMarker"] {
        close(
            run(&pages[1], marker).bbox.y - run(&pages[0], marker).bbox.y,
            px(600),
            "다음 내용 전진에 위아래 외곽을 한 번 계상",
        );
    }
    close(
        run(&pages[1], "  ").bbox.x - run(&pages[0], "  ").bbox.x,
        px(900),
        "원문 공백의 가로 전진도 완전 frame 소비",
    );
    let left = unique(cells(positive), "FirstCell");
    let right_cell = unique(cells(positive), "DerivedWidthMarker");
    close(left.bbox.x - positive.bbox.x, px(300), "최종 셀 왼쪽 외곽");
    close(
        right_cell.bbox.x - left.bbox.x,
        px(10300),
        "10000HU 열과 내부 300HU 간격의 실제 원점 차이",
    );
    close(
        right_cell.bbox.width,
        px(10000),
        "마지막 열의 authored 너비 보존",
    );

    close(
        right(positive) - right(right_cell),
        px(300),
        "최종 셀 오른쪽 외곽",
    );
    close(
        bottom(positive) - bottom(left),
        px(300),
        "최종 셀 아래 외곽",
    );
}

#[test]
fn complete_grid_keeps_the_same_frame_for_three_declared_widths() {
    let pages = pages();
    let reference = tables(&pages[1])[0];
    for page in &pages[2..4] {
        let table = tables(page)[0];
        close(
            table.bbox.width,
            reference.bbox.width,
            "선언 0/20000/20900에도 같은 완전 셀 격자",
        );
        close(
            run(page, "DerivedWidthMarker").bbox.x,
            run(&pages[1], "DerivedWidthMarker").bbox.x,
            "선언 너비는 실제 열 원점을 바꾸지 않음",
        );
        close(
            run(page, "  ").bbox.x,
            run(&pages[1], "  ").bbox.x,
            "선언 너비는 실제 공백 전진을 바꾸지 않음",
        );
        close(
            run(page, "FollowingMarker").bbox.y,
            run(&pages[1], "FollowingMarker").bbox.y,
            "동일한 뒤 본문 위치",
        );
    }
}

#[test]
fn two_authored_rows_keep_their_height_and_one_internal_gap() {
    let pages = pages();
    let table = tables(&pages[4])[0];
    let first = unique(cells(table), "FirstCell");
    let second_row = unique(cells(table), "Row2Left");
    close(
        second_row.bbox.y - first.bbox.y,
        px(7500),
        "7200HU 행을 줄이지 않고 내부 300HU 간격 보존",
    );
    close(
        second_row.bbox.height,
        px(7200),
        "마지막 authored 행 높이 보존",
    );
    close(
        table.bbox.height,
        px(15300),
        "두 행과 내부 하나·외곽 둘 간격",
    );
    close(
        run(&pages[4], "FollowingMarker").bbox.y - run(&pages[1], "FollowingMarker").bbox.y,
        px(7500),
        "추가 행과 내부 간격만큼 뒤 본문 전진",
    );
}

#[test]
fn merged_second_row_does_not_add_the_spanned_column_gap_twice() {
    let pages = pages();
    let grid = tables(&pages[4])[0];
    let merged_table = tables(&pages[5])[0];
    close(
        merged_table.bbox.width,
        grid.bbox.width,
        "병합행은 기존 격자 너비를 늘리지 않음",
    );
    let merged = unique(cells(merged_table), "Row2MergedCell");
    close(
        merged.bbox.width,
        px(20300),
        "병합 20300HU는 내부 300HU를 이미 포함",
    );
    let RenderNodeType::TableCell(owner) = &merged.node_type else {
        unreachable!()
    };
    assert_eq!(
        (owner.row, owner.col, owner.row_span, owner.col_span),
        (1, 0, 1, 2),
        "원문 둘째 행 두 열 병합 소유"
    );
    close(
        merged.bbox.y - unique(cells(merged_table), "FirstCell").bbox.y,
        px(7500),
        "병합 후에도 authored 행과 내부 간격 보존",
    );
    close(
        run(&pages[5], "FollowingMarker").bbox.y,
        run(&pages[4], "FollowingMarker").bbox.y,
        "병합 여부와 무관한 뒤 본문 위치",
    );
}

#[test]
fn outer_margins_are_additional_to_spacing_and_consumed_once() {
    let pages = pages();
    let base = tables(&pages[1])[0];
    let margin = tables(&pages[6])[0];
    close(
        run(&pages[6], "FirstCell").bbox.x - run(&pages[6], "BeforeMarker").bbox.x,
        px(450),
        "150HU 바깥여백과 300HU 외곽 밴드는 별개",
    );
    close(margin.bbox.x - base.bbox.x, px(150), "왼쪽 바깥여백 한 번");
    close(
        margin.bbox.width,
        base.bbox.width,
        "바깥여백은 셀 frame 너비에 중복 포함하지 않음",
    );
    close(
        run(&pages[6], "  ").bbox.x - run(&pages[1], "  ").bbox.x,
        px(300),
        "좌우 바깥여백은 가로 전진에 각각 한 번",
    );
    close(
        host(&pages[6]).bbox.width - host(&pages[1]).bbox.width,
        px(300),
        "host 할당 너비에도 여백 한 번",
    );
    close(
        run(&pages[6], "FollowingMarker").bbox.y,
        run(&pages[1], "FollowingMarker").bbox.y,
        "좌우 여백은 세로 전진을 바꾸지 않음",
    );
}

#[test]
fn unequal_tables_share_one_host_line_without_summing_heights() {
    let pages = pages();
    let page = &pages[7];
    let objects = tables(page);
    let line = host(page);
    close(
        line.bbox.height,
        px(7800 + 150 + 150),
        "같은 줄은 큰 전체 frame과 위아래 여백을 한 번 예약",
    );
    contained(line, objects[0]);
    contained(line, objects[1]);
    assert!(
        right(objects[0]) <= objects[1].bbox.x,
        "두 표의 가로 순서 보존"
    );
    assert!(
        objects[1].bbox.y < bottom(objects[0]),
        "둘째 표를 새 줄로 밀지 않음"
    );
    assert!(
        line.bbox.height < objects[0].bbox.height + objects[1].bbox.height,
        "같은 줄의 두 높이를 합산하지 않음"
    );
    close(
        run(page, "  ").bbox.x,
        right(objects[0]),
        "원문 공백은 첫 표 바로 뒤",
    );
    close(
        objects[1].bbox.x,
        right(run(page, "  ")),
        "둘째 표는 실제 두 공백 전진 바로 뒤",
    );
    close(
        run(page, "FollowingMarker").bbox.y - bottom(line),
        px(600),
        "뒤 본문은 공유 줄 끝과 원문 줄간격 뒤",
    );
    assert!(
        objects[0].bbox.y < objects[1].bbox.y,
        "높이가 다른 표는 같은 줄에서 각자의 세로 원점을 유지"
    );
}
