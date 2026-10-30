//! 독립 한컴 PDF의 쪽 및 셀 소유와 생성 줄의 예약/배치를 공개 API로 검증한다.
#![cfg(not(target_arch = "wasm32"))]

use rhwp::{renderer::font_environment::FontEnvironment, DocumentCore};
use serde_json::Value;

const HWP: &[u8] = include_bytes!("../fixtures/generated_cell_continuation/long-cell.hwp");
const HWPX: &[u8] = include_bytes!("../fixtures/generated_cell_continuation/long-cell.hwpx");
const ENVIRONMENT: &str = include_str!("../fixtures/generated_cell_continuation/environment.json");

fn nodes<'a>(node: &'a Value, kind: &str, out: &mut Vec<&'a Value>) {
    if node["type"] == kind {
        out.push(node);
    }
    if let Some(children) = node["children"].as_array() {
        for child in children {
            nodes(child, kind, out);
        }
    }
}

fn descendants<'a>(node: &'a Value, kind: &str) -> Vec<&'a Value> {
    let mut result = Vec::new();
    nodes(node, kind, &mut result);
    result
}

fn text(node: &Value) -> String {
    descendants(node, "TextRun")
        .iter()
        .map(|run| run["text"].as_str().expect("텍스트 run 문자열"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn segments(node: &Value) -> Vec<usize> {
    text(node)
        .split_whitespace()
        .filter_map(|word| word.strip_prefix("segment"))
        .map(|digits| digits.parse().expect("세 자리 segment 번호"))
        .collect()
}

fn number(value: &Value, key: &str) -> f64 {
    value[key].as_f64().expect("기하 숫자 필드")
}

fn bottom(node: &Value) -> f64 {
    number(&node["bbox"], "y") + number(&node["bbox"], "h")
}

fn contained(outer: &Value, inner: &Value, context: &str) {
    let a = &outer["bbox"];
    let b = &inner["bbox"];
    // RenderTree JSON rounds box coordinates to one decimal place.
    let epsilon = 0.11;
    assert!(
        number(b, "x") >= number(a, "x") - epsilon
            && number(b, "y") >= number(a, "y") - epsilon
            && number(b, "x") + number(b, "w") <= number(a, "x") + number(a, "w") + epsilon
            && bottom(inner) <= bottom(outer) + epsilon,
        "{context}: 안쪽 상자 {b}가 바깥 상자 {a}에 포함되어야 한다"
    );
}

fn check_continuation(bytes: &[u8], native_no_lineseg: bool) {
    let mut core = DocumentCore::from_bytes(bytes).expect("합성 원본을 열어야 한다");
    if native_no_lineseg {
        let imported = rhwp::parse_document(bytes).expect("배치 전 원문 파싱");
        let table = imported.sections[0].paragraphs[0]
            .controls
            .iter()
            .find_map(|control| match control {
                rhwp::model::control::Control::Table(table) => Some(table),
                _ => None,
            })
            .expect("원문 첫 문단의 표");
        assert!(
            table
                .cells
                .iter()
                .filter(|cell| cell.col == 1)
                .all(|cell| cell
                    .paragraphs
                    .iter()
                    .all(|paragraph| paragraph.line_segs.is_empty())),
            "HWP는 저장 LineSeg 없는 실제 생성 줄 경로여야 한다"
        );
    }
    core.set_font_environment(Some(
        FontEnvironment::from_json(ENVIRONMENT).expect("검증된 HCR 호출자 환경"),
    ))
    .expect("호출자 글꼴 환경 선택");
    assert_eq!(
        core.page_count(),
        7,
        "독립 한컴 기준과 같이 정확히 7쪽이어야 한다"
    );
    let plan = core.dump_page_items_json(None);
    let mut all_segments = Vec::new();
    let mut previous_end = serde_json::json!([]);
    for page in 0..7 {
        let root: Value = serde_json::from_str(
            &core
                .build_page_render_tree(page)
                .expect("쪽 트리 생성")
                .root
                .to_json(),
        )
        .expect("쪽 트리 JSON");
        let tables = descendants(&root, "Table");
        assert_eq!(
            tables.len(),
            1,
            "각 쪽은 원문 표의 조각 하나를 소유해야 한다"
        );
        let table = tables[0];
        assert_eq!(table["pi"], 0, "모든 표 조각은 첫 원문 문단 소유여야 한다");
        let bodies = descendants(&root, "Body");
        assert_eq!(bodies.len(), 1, "본문 하나를 찾아야 한다");
        contained(bodies[0], table, "표 조각은 본문 내부");
        let cells = descendants(table, "Cell");
        assert_eq!(cells.len(), 2, "이어받기에서도 두 열 소유를 보존해야 한다");
        let left = cells
            .iter()
            .find(|cell| cell["col"] == 0)
            .expect("왼쪽 이름 셀");
        assert_eq!(left["row"], 0, "왼쪽 셀도 원문 0행 소유여야 한다");
        assert_eq!(
            text(left).trim(),
            if page == 0 { "Label0" } else { "" },
            "완료된 왼쪽 셀은 이어받기에 Label0를 다시 그리면 안 된다"
        );
        let cell = cells
            .iter()
            .find(|cell| cell["col"] == 1)
            .expect("오른쪽 긴 셀");
        assert_eq!(cell["row"], 0, "모든 segment는 원문 0행 소유여야 한다");
        let expected: Vec<_> = if page < 6 {
            (page as usize * 32 + 1..=page as usize * 32 + 32).collect()
        } else {
            (193..=200).collect()
        };
        assert_eq!(
            segments(cell),
            expected,
            "각 쪽 오른쪽 셀의 정확한 segment 소유"
        );
        assert_eq!(
            segments(&root),
            expected,
            "다른 셀/본문에 segment 중복이 없어야 한다"
        );
        all_segments.extend(segments(cell));
        let page_text = text(&root);
        assert_eq!(
            page_text.matches("Label0").count(),
            usize::from(page == 0),
            "Label0는 첫 쪽에만 한 번"
        );
        assert_eq!(
            page_text.matches("Following body marker").count(),
            usize::from(page == 6),
            "뒤 본문은 마지막 쪽에만 한 번"
        );
        for owned_cell in &cells {
            contained(table, owned_cell, "셀은 실제 표 외곽 내부");
            for line in descendants(owned_cell, "TextLine") {
                contained(owned_cell, line, "글줄은 소유 셀 내부");
            }
        }
        let lines = descendants(cell, "TextLine");
        assert_eq!(
            lines.len(),
            expected.len() / 2,
            "독립 PDF처럼 각 줄에 두 segment"
        );
        for (line, pair) in lines.iter().zip(expected.chunks(2)) {
            assert_eq!(
                segments(line),
                pair,
                "줄 소유 유닛의 누락/중복이 없어야 한다"
            );
        }
        for pair in lines.windows(2) {
            assert!(
                bottom(pair[0]) <= number(&pair[1]["bbox"], "y") + 0.11,
                "같은 셀의 글줄이 겹치면 안 된다"
            );
        }
        let column = &plan[page as usize]["columns"][0];
        let items = column["items"].as_array().expect("예약 항목 배열");
        let table_items: Vec<_> = items
            .iter()
            .filter(|item| item["kind"] == "partialTable")
            .collect();
        assert_eq!(table_items.len(), 1, "쪽마다 표 조각 예약 하나");
        let item = table_items[0];
        assert_eq!(item["startRow"], 0, "시작 행 소유");
        assert_eq!(item["endRow"], 1, "끝 행은 원문 한 행 직후");
        assert_eq!(item["isContinuation"], page != 0, "첫 조각과 이어받기 구분");
        assert_eq!(
            item["startCut"], previous_end,
            "앞쪽 끝 컷과 다음 시작 컷이 같아야 한다"
        );
        if page < 6 {
            let end = &item["endCut"];
            assert_eq!(
                end,
                &serde_json::json!([1, (page + 1) * 16]),
                "첫/중간 컷은 왼쪽 완료와 오른쪽 16줄 증가를 함께 소유해야 한다"
            );
            assert!(
                (number(column, "usedHeight") - number(&table["bbox"], "h")).abs() <= 0.11,
                "페이지가 예약한 공간과 실제 표 외곽 높이가 같아야 한다"
            );
        } else {
            assert_eq!(
                item["endCut"],
                serde_json::json!([]),
                "마지막 유닛 뒤에 이월 컷이 없어야 한다"
            );
            let markers: Vec<_> = descendants(&root, "TextLine")
                .into_iter()
                .filter(|line| text(line).contains("Following body marker"))
                .collect();
            assert_eq!(markers.len(), 1, "뒤 본문은 표 밖의 한 글줄");
            contained(bodies[0], markers[0], "뒤 본문은 본문 내부");
            assert!(
                number(&markers[0]["bbox"], "y") >= bottom(table) - 0.11,
                "뒤 본문은 실제 마지막 표 외곽 아래여야 한다"
            );
            assert!(
                !text(table).contains("Following body marker"),
                "뒤 본문은 표 소유가 아니어야 한다"
            );
        }
        previous_end = item["endCut"].clone();
    }
    assert_eq!(
        all_segments,
        (1..=200).collect::<Vec<_>>(),
        "전체 200개 유닛이 정확히 한 번 순서대로 보존되어야 한다"
    );
    assert!(
        core.build_page_render_tree(7).is_err(),
        "마지막 유닛 뒤 빈 8쪽이 없어야 한다"
    );
}

#[test]
fn generated_hwpx_preserves_hancom_page_and_cell_ownership() {
    check_continuation(HWPX, false);
}

#[test]
fn native_hwp_without_saved_lines_preserves_hancom_page_and_cell_ownership() {
    check_continuation(HWP, true);
}

/// 저장 왕복은 같은 셀의 전체 내용과 쪽별 소유, 뒤 본문 소속을 보존한다.
#[derive(Debug, PartialEq)]
struct SavedCellOwnership {
    pages: u32,
    fragments: Vec<(u32, String)>,
    following_pages: Vec<u32>,
}

fn compact_text(value: &str) -> String {
    value.chars().filter(|c| !c.is_whitespace()).collect()
}

fn source_cell(core: &DocumentCore, paragraph: usize, row: u16) -> (usize, usize) {
    core.document().sections[0].paragraphs[paragraph]
        .controls
        .iter()
        .enumerate()
        .find_map(|(control, item)| match item {
            rhwp::model::control::Control::Table(table) => table
                .cells
                .iter()
                .position(|cell| cell.row == row && cell.col == 1)
                .map(|cell| (control, cell)),
            _ => None,
        })
        .expect("원문 긴 셀의 논리 소유")
}

fn saved_cell_ownership(core: &DocumentCore, paragraph: usize, row: u16) -> SavedCellOwnership {
    let (control, cell) = source_cell(core, paragraph, row);
    let rhwp::model::control::Control::Table(table) =
        &core.document().sections[0].paragraphs[paragraph].controls[control]
    else {
        unreachable!("원문 소유 컨트롤은 표여야 한다")
    };
    let expected: String = table.cells[cell]
        .paragraphs
        .iter()
        .map(|p| compact_text(&p.text))
        .collect();
    let following = compact_text(&core.document().sections[0].paragraphs[paragraph + 1].text);
    assert!(
        !following.is_empty(),
        "표 뒤 본문의 비교 내용이 있어야 한다"
    );
    let mut ownership = SavedCellOwnership {
        pages: core.page_count(),
        fragments: Vec::new(),
        following_pages: Vec::new(),
    };
    for page in 0..core.page_count() {
        let root: Value = serde_json::from_str(
            &core
                .build_page_render_tree(page)
                .expect("쪽 트리 생성")
                .root
                .to_json(),
        )
        .expect("쪽 트리 JSON");
        for table in descendants(&root, "Table")
            .into_iter()
            .filter(|t| t["pi"] == paragraph)
        {
            let cells: Vec<_> = descendants(table, "Cell")
                .into_iter()
                .filter(|c| c["row"] == row && c["col"] == 1)
                .collect();
            assert!(
                cells.len() <= 1,
                "한 표 조각에 같은 셀을 중복 배치하면 안 된다"
            );
            for cell in cells {
                ownership.fragments.push((page, compact_text(&text(cell))));
            }
        }
        let page_text = compact_text(&text(&root));
        if page + 1 == core.page_count() {
            assert!(
                !page_text.is_empty(),
                "텍스트 원본의 마지막 유효 페이지에 실제 내용이 남아야 한다"
            );
        }
        if page_text.contains(&following) {
            ownership.following_pages.push(page);
        }
    }
    let complete: String = ownership
        .fragments
        .iter()
        .map(|(_, content)| content.as_str())
        .collect();
    assert_eq!(
        complete, expected,
        "셀 전체 내용은 원문 순서대로 한 번씩 보존되어야 한다"
    );
    let last = ownership.fragments.last().expect("긴 셀의 마지막 조각").0;
    assert!(
        ownership.following_pages.iter().any(|&page| page >= last),
        "마지막 표 조각 뒤에 후속 본문이 남아야 한다"
    );
    ownership
}

fn caller_environment(core: &mut DocumentCore) {
    core.set_font_environment(Some(
        FontEnvironment::from_json(ENVIRONMENT).expect("검증된 호출자 글꼴 환경"),
    ))
    .expect("호출자 글꼴 환경 선택");
}

/// HWPX 계보의 HWP를 HWPX로 저장해도 독립 PDF의 7쪽 소유를 유지한다.
#[test]
fn hwpx_origin_hwp_preserves_ownership_through_hwpx_export() {
    let mut core = DocumentCore::from_bytes(HWP).expect("HWPX 계보 HWP 열기");
    caller_environment(&mut core);
    let live = saved_cell_ownership(&core, 0, 0);
    assert_eq!(live.pages, 7, "독립 PDF의 전체 7쪽");
    let bytes = core.export_hwpx_native().expect("HWPX 공개 저장");
    check_continuation(&bytes, false);
    let mut reopened = DocumentCore::from_bytes(&bytes).expect("저장한 HWPX 다시 열기");
    caller_environment(&mut reopened);
    assert!(
        reopened.document().layout_profile().hwpx_stored_layout(),
        "HWPX 원본 계보는 HWP5 저장 프레임 계약으로 바뀌면 안 된다"
    );
    assert_eq!(
        saved_cell_ownership(&reopened, 0, 0),
        live,
        "변환 전후의 전체 셀 내용과 쪽별 소유 및 후속 본문 소속"
    );
}

/// 편집한 HWPX 계보의 HWP도 두 공개 저장 형식에서 같은 조각을 소유한다.
#[test]
fn edited_hwpx_origin_hwp_preserves_ownership_through_both_exports() {
    let mut core = DocumentCore::from_bytes(HWP).expect("HWPX 계보 HWP 열기");
    caller_environment(&mut core);
    let (control, cell) = source_cell(&core, 0, 0);
    core.insert_text_in_cell_native(
        0,
        0,
        control,
        cell,
        0,
        0,
        &"수정된 검증 문장입니다. ".repeat(30),
    )
    .expect("긴 셀 공개 편집");
    let live = saved_cell_ownership(&core, 0, 0);
    for bytes in [
        core.export_hwp_with_adapter().expect("HWP 공개 저장"),
        core.export_hwpx_native().expect("HWPX 공개 저장"),
    ] {
        let mut reopened = DocumentCore::from_bytes(&bytes).expect("저장본 다시 열기");
        caller_environment(&mut reopened);
        assert_eq!(
            saved_cell_ownership(&reopened, 0, 0),
            live,
            "편집한 긴 셀의 전체 내용과 정확한 조각 순서 및 뒤 본문 소속"
        );
    }
}

/// 실제 native HWP5의 현재 생성 줄은 편집 세션과 저장 재열기에서 같은 컷을 쓴다.
/// 내용 불변 교체와 성장 편집을 구분하며 설치 글꼴이나 픽셀 좌표를 요구하지 않는다.
#[test]
fn native_generated_cell_preserves_current_ownership_after_edit_save_reopen() {
    let bytes =
        include_bytes!("../../samples/issue6776/78494-virtual-convergence-industry-decree.hwpx");
    for growth in [false, true] {
        let mut core = DocumentCore::from_bytes(bytes).expect("실제 native HWP5 원본 열기");
        assert!(
            core.document().layout_profile().native_hwp5_layout(),
            "확장자와 별개로 실제 입력은 native HWP5여야 한다"
        );
        let original = (!growth).then(|| saved_cell_ownership(&core, 229, 1));
        let (control, cell) = source_cell(&core, 229, 1);
        if growth {
            core.insert_text_in_cell_native(
                0,
                229,
                control,
                cell,
                0,
                0,
                &"수정된 검증 문장입니다. ".repeat(30),
            )
            .expect("실제 셀의 공개 성장 편집");
        } else {
            core.replace_text_in_cell_native_deferred_pagination(
                0, 229, control, cell, 0, 0, 1, "제",
            )
            .expect("같은 첫 글자로 공개 교체");
            core.flush_deferred_pagination();
        }
        let live = saved_cell_ownership(&core, 229, 1);
        if let Some(original) = original {
            assert_eq!(
                live, original,
                "내용 불변 편집은 원본 쪽 소유를 바꾸면 안 된다"
            );
        }
        for bytes in [
            core.export_hwp_with_adapter().expect("HWP 공개 저장"),
            core.export_hwpx_native().expect("HWPX 공개 저장"),
        ] {
            let reopened = DocumentCore::from_bytes(&bytes).expect("편집 저장본 다시 열기");
            assert_eq!(
                saved_cell_ownership(&reopened, 229, 1),
                live,
                "native 생성 셀의 전체 내용과 정확한 쪽별 조각 및 후속 본문 소속"
            );
        }
    }
}
