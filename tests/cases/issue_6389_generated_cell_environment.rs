//! 생성 셀 줄의 소유 환경과 paint가 같은 분할을 소비하는지 공개 API로 검증한다.
//! 독립 한컴 전체 쪽 근거와 정확한 입력 해시는 fixture의 provenance.json에 있다.
#![cfg(not(target_arch = "wasm32"))]

use rhwp::{renderer::font_environment::FontEnvironment, DocumentCore};
use serde_json::Value;

const HWP: &[u8] = include_bytes!("../fixtures/issue_6389_generated_cell_environment/edited.hwp");
const HWPX: &[u8] = include_bytes!("../fixtures/issue_6389_generated_cell_environment/edited.hwpx");
const ENVIRONMENT: &str =
    include_str!("../fixtures/issue_6389_generated_cell_environment/environment.json");

fn environment() -> FontEnvironment {
    FontEnvironment::from_json(ENVIRONMENT).expect("검증된 HCR 환경 A를 읽어야 한다")
}

fn trees(core: &DocumentCore) -> Vec<Value> {
    (0..core.page_count())
        .map(|page| {
            let tree = core
                .build_page_render_tree(page)
                .expect("쪽 트리를 만들어야 한다");
            serde_json::from_str(&tree.root.to_json()).expect("쪽 트리 JSON을 읽어야 한다")
        })
        .collect()
}

fn first_cell(node: &Value) -> Option<&Value> {
    if node["type"] == "Cell" {
        return Some(node);
    }
    node["children"].as_array()?.iter().find_map(first_cell)
}

fn text(node: &Value) -> String {
    let mut result = node["text"].as_str().unwrap_or("").to_owned();
    if let Some(children) = node["children"].as_array() {
        for child in children {
            result.push_str(&text(child));
        }
    }
    result
}

fn boxes(node: &Value, kind: &str, result: &mut Vec<(String, Value)>) {
    if node["type"] == kind {
        result.push((text(node), node["bbox"].clone()));
        return;
    }
    if let Some(children) = node["children"].as_array() {
        for child in children {
            boxes(child, kind, result);
        }
    }
}

fn assert_hancom_first_cell(tree: &[Value]) {
    assert_eq!(tree.len(), 2, "한컴과 같은 전체 2쪽을 보존해야 한다");
    let cell = first_cell(&tree[0]).expect("첫 쪽에 첫 셀이 있어야 한다");
    let mut lines = Vec::new();
    boxes(cell, "TextLine", &mut lines);
    assert_eq!(lines.len(), 16, "한컴 첫 셀의 16줄을 재현해야 한다");
    assert_eq!(
        lines[0].0.split_whitespace().collect::<Vec<_>>(),
        ["Edited"; 4],
        "한컴 첫 줄은 Edited 네 단어여야 한다"
    );
}

#[test]
fn selected_environment_reflows_generated_rows_and_matches_native_geometry() {
    let mut cells = Vec::new();
    for bytes in [HWP, HWPX] {
        let mut core = DocumentCore::from_bytes(bytes).expect("검증된 입력을 열어야 한다");
        let hwp = core.export_hwp_native().expect("선택 전 HWP 내보내기");
        let hwpx = core.export_hwpx_native().expect("선택 전 HWPX 내보내기");
        assert!(core
            .set_font_environment(Some(environment()))
            .expect("환경 A 선택"));
        let tree = trees(&core);
        assert_hancom_first_cell(&tree);
        assert_eq!(
            core.export_hwp_native().expect("선택 후 HWP 내보내기"),
            hwp,
            "글꼴 선택은 원본 HWP 바이트를 바꾸면 안 된다"
        );
        assert_eq!(
            core.export_hwpx_native().expect("선택 후 HWPX 내보내기"),
            hwpx,
            "글꼴 선택은 원본 HWPX 바이트를 바꾸면 안 된다"
        );
        cells.push(first_cell(&tree[0]).expect("첫 셀").clone());
    }
    assert_eq!(
        cells[0]["bbox"], cells[1]["bbox"],
        "두 형식의 셀 기하가 같아야 한다"
    );
    for kind in ["TextLine", "TextRun"] {
        let mut native = Vec::new();
        let mut generated = Vec::new();
        boxes(&cells[0], kind, &mut native);
        boxes(&cells[1], kind, &mut generated);
        assert_eq!(
            native, generated,
            "두 형식의 {kind} 내용과 기하가 같아야 한다"
        );
    }
}

#[test]
fn restoring_pre_environment_snapshot_and_replacing_document_keep_selected_rows() {
    let mut core = DocumentCore::from_bytes(HWPX).expect("검증된 HWPX를 열어야 한다");
    let imported = core.document().clone();
    let hwp = core.export_hwp_native().expect("원본 HWP 내보내기");
    let hwpx = core.export_hwpx_native().expect("원본 HWPX 내보내기");
    let snapshot = core.save_snapshot_native();
    core.set_font_environment(Some(environment()))
        .expect("환경 A 선택");
    let selected = trees(&core);
    assert_hancom_first_cell(&selected);
    core.restore_snapshot_native(snapshot)
        .expect("환경 선택 전 스냅샷 복원");
    assert_eq!(
        trees(&core),
        selected,
        "스냅샷 복원은 현재 A의 줄과 paint를 유지해야 한다"
    );
    core.set_document(imported);
    assert_eq!(
        trees(&core),
        selected,
        "문서 교체는 현재 A의 줄과 paint를 유지해야 한다"
    );
    assert!(
        !core
            .set_font_environment(Some(environment()))
            .expect("같은 환경 A 재선택"),
        "같은 환경은 다시 계산하지 않아야 한다"
    );
    assert_eq!(
        trees(&core),
        selected,
        "같은 환경 재선택은 전체 쪽 트리를 바꾸면 안 된다"
    );
    assert_eq!(
        core.export_hwp_native().expect("복원 후 HWP 내보내기"),
        hwp,
        "복원은 원본 HWP 바이트를 보존해야 한다"
    );
    assert_eq!(
        core.export_hwpx_native().expect("복원 후 HWPX 내보내기"),
        hwpx,
        "복원은 원본 HWPX 바이트를 보존해야 한다"
    );
}
