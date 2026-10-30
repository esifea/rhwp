//! 누락된 원문 face의 호출자 선택, 실제 메트릭과 세션 수명을 검증한다.
#![cfg(not(target_arch = "wasm32"))]

use rhwp::{renderer::font_environment::FontEnvironment, DocumentCore};
use serde_json::{json, Value};

const HWP: &[u8] = include_bytes!("../fixtures/generated_cell_continuation/long-cell.hwp");
const HWPX: &[u8] = include_bytes!("../fixtures/generated_cell_continuation/long-cell.hwpx");

fn environment(face: Option<&str>) -> FontEnvironment {
    let mut declaration = json!({
        "id":"same-fallback-session",
        "substitutions":{},
        "trueTypeFaces":["HCR Dotum"]
    });
    if let Some(face) = face {
        declaration["fallbackFace"] = json!(face);
    }
    FontEnvironment::from_json(&declaration.to_string()).expect("유효한 호출자 환경 선언")
}

fn trees(core: &DocumentCore) -> Vec<Value> {
    (0..core.page_count())
        .map(|page| {
            serde_json::from_str(
                &core
                    .build_page_render_tree(page)
                    .expect("쪽 트리 생성")
                    .root
                    .to_json(),
            )
            .expect("쪽 트리 JSON")
        })
        .collect()
}

fn record(core: &DocumentCore, character: &str) -> Value {
    let trace: Value = serde_json::from_str(
        &core
            .get_font_decision_trace_native(0, r#"{"maxCharacters":256}"#)
            .expect("공개 글꼴 결정 추적"),
    )
    .expect("글꼴 추적 JSON");
    trace["records"]
        .as_array()
        .expect("글꼴 레코드 배열")
        .iter()
        .find(|record| record["source"]["character"] == character)
        .expect("검사할 문자 레코드")
        .clone()
}

fn fonts_used(core: &DocumentCore) -> Value {
    let info: Value = serde_json::from_str(&core.get_document_info()).expect("문서 글꼴 목록 JSON");
    info["fontsUsed"].clone()
}

fn exports(core: &DocumentCore) -> (Vec<u8>, Vec<u8>) {
    (
        core.export_hwp_native().expect("HWP 원문 내보내기"),
        core.export_hwpx_native().expect("HWPX 원문 내보내기"),
    )
}

#[test]
fn fallback_declaration_is_optional_and_legacy_serialization_stays_exact() {
    let legacy = r#"{"id":"legacy","substitutions":{}}"#;
    let parsed = FontEnvironment::from_json(legacy).expect("기존 선언 허용");
    assert_eq!(
        serde_json::to_string(&parsed).expect("기존 선언 직렬화"),
        legacy,
        "fallback 생략은 기존 JSON 바이트를 보존해야 한다"
    );
    let explicit = environment(Some("HCR Dotum"));
    let serialized = serde_json::to_value(&explicit).expect("명시적 선언 직렬화");
    assert_eq!(
        serialized["fallbackFace"], "HCR Dotum",
        "단일 face 선언 보존"
    );
    assert_eq!(
        FontEnvironment::from_json(&serialized.to_string()).expect("명시적 선언 재읽기"),
        explicit,
        "선택한 fallback을 왕복 보존해야 한다"
    );
}

#[test]
fn fallback_rejects_empty_whitespace_css_lists_controls_and_oversize_faces() {
    for face in [
        "".to_owned(),
        " ".to_owned(),
        " HCR Dotum".to_owned(),
        "HCR Dotum ".to_owned(),
        "HCR Dotum,serif".to_owned(),
        "a'b".to_owned(),
        "a\"b".to_owned(),
        "a\\b".to_owned(),
        "a;b".to_owned(),
        "a{b".to_owned(),
        "a}b".to_owned(),
        "a\nb".to_owned(),
        "a\tb".to_owned(),
        "a\0b".to_owned(),
        "a\u{7f}b".to_owned(),
        "a".repeat(257),
        "가".repeat(86),
    ] {
        let declaration = json!({"id":"invalid","substitutions":{},"fallbackFace":face});
        assert!(
            FontEnvironment::from_json(&declaration.to_string()).is_err(),
            "잘못된 fallback 단일 face를 거절해야 한다: {face:?}"
        );
    }
    assert!(
        FontEnvironment::from_json(r#"{"id":"invalid","substitutions":{},"fallbackFace":12}"#)
            .is_err(),
        "문자열이 아닌 fallback을 거절해야 한다"
    );
    assert!(
        FontEnvironment::from_json(
            &json!({"id":"boundary","substitutions":{},"fallbackFace":"a".repeat(256)}).to_string()
        )
        .is_ok(),
        "256바이트 경계의 유효한 단일 face는 허용해야 한다"
    );
}

#[test]
fn missing_source_slot_selects_exact_hcr_metrics_and_preserves_source_exports() {
    for (bytes, implicit_fonts, implicit_width_source, implicit_space_source) in [
        (HWP, json!([]), "heuristicHalfwidth", "heuristicHalfwidth"),
        (
            HWPX,
            json!(["한컴바탕"]),
            "embeddedMetric",
            "metricHalfSpace",
        ),
    ] {
        let mut core = DocumentCore::from_bytes(bytes).expect("글꼴 슬롯 없는 원본 읽기");
        assert!(
            core.document()
                .doc_info
                .font_faces
                .iter()
                .all(Vec::is_empty),
            "원본 모든 언어 face 슬롯이 비어 있어야 한다"
        );
        assert_eq!(
            fonts_used(&core),
            implicit_fonts,
            "환경 선택 전에는 형식별 결측 글꼴 기본값을 보존해야 한다"
        );
        let original = exports(&core);
        let before = record(&core, "s");
        assert!(before["document"]["face"].is_null(), "원문 face 선언 없음");
        assert_eq!(
            before["layoutMetric"]["widthSource"], implicit_width_source,
            "선택 전에는 HWP 휴리스틱과 HWPX 기본 글꼴 메트릭을 구분해야 한다"
        );
        assert_eq!(
            before["layoutMetric"]["baseAdvanceHwpunit"], 500,
            "선택 전 s 폭은 기존 반각 또는 기본 한컴바탕의 0.5em이다"
        );
        if bytes == HWPX {
            // layoutName은 원본 IR을 조회하고, 기본 face는 표시용 조회 문맥에만 있다.
            assert_eq!(
                before["layoutMetric"]["requestedFace"], "한컴바탕",
                "HWPX 결측 영문 슬롯은 표시용 기본 face의 메트릭을 선택해야 한다"
            );
        }
        let space = record(&core, " ");
        assert_eq!(
            space["layoutMetric"]["widthSource"], implicit_space_source,
            "선택 전 공백도 형식별 기본 측정 경로를 따라야 한다"
        );
        assert_eq!(
            space["layoutMetric"]["baseAdvanceHwpunit"], 500,
            "useFontSpace=0인 원문 공백은 기본 face와 무관하게 반각이다"
        );
        core.set_font_environment(Some(environment(Some("HCR Dotum"))))
            .expect("HCR fallback 선택");
        assert_eq!(
            fonts_used(&core),
            json!(["HCR Dotum"]),
            "호스트 글꼴 탐색 목록은 실제로 선택한 fallback을 포함해야 한다"
        );
        for (character, advance) in [("s", 513), ("e", 567), ("m", 919), ("0", 550)] {
            let after = record(&core, character);
            assert!(
                after["document"]["face"].is_null(),
                "원문 face는 계속 미선언이어야 한다"
            );
            assert_eq!(
                after["layoutName"]["normalizedFace"], "HCR Dotum",
                "레이아웃은 선택한 face를 사용해야 한다"
            );
            assert_eq!(
                after["paint"]["native"]["requested"], "HCR Dotum",
                "paint와 측정의 face가 같아야 한다"
            );
            assert_eq!(
                after["layoutMetric"]["widthSource"], "embeddedMetric",
                "caller ID만 바꾸지 말고 실제 메트릭을 선택해야 한다"
            );
            assert_eq!(
                after["layoutMetric"]["baseAdvanceHwpunit"], advance,
                "공개 HANDotum.ttf hmtx의 독립 문자 폭"
            );
        }
        assert!(
            core.document()
                .doc_info
                .font_faces
                .iter()
                .all(Vec::is_empty),
            "세션 fallback으로 원문 face를 만들면 안 된다"
        );
        assert_eq!(
            exports(&core),
            original,
            "세션 선택은 두 형식의 원문 내보내기 바이트를 정확히 보존해야 한다"
        );
    }
}

#[test]
fn same_id_fallback_changes_survive_snapshot_and_document_replacement() {
    for (bytes, implicit_fonts) in [(HWP, json!([])), (HWPX, json!(["한컴바탕"]))] {
        let mut core = DocumentCore::from_bytes(bytes).expect("원본 읽기");
        let document = core.document().clone();
        let snapshot = core.save_snapshot_native();
        let original = exports(&core);
        let initial = trees(&core);
        let mut selected_states = Vec::new();
        for face in [Some("HCR Dotum"), Some("HCR Batang"), None] {
            let selected_environment = face.map(|face| environment(Some(face)));
            assert!(
                core.set_font_environment(selected_environment.clone())
                    .expect("같은 ID의 다른 fallback 선택"),
                "A/B/None 전환은 재구성을 수행해야 한다"
            );
            let expected_fonts = face.map_or_else(|| implicit_fonts.clone(), |face| json!([face]));
            assert_eq!(
                fonts_used(&core),
                expected_fonts,
                "A/B/None 전환은 실제 선택한 글꼴 탐색 목록을 갱신해야 한다"
            );
            let expected = trees(&core);
            let mut fresh = DocumentCore::from_bytes(bytes).expect("같은 원본 새 세션");
            fresh
                .set_font_environment(selected_environment.clone())
                .expect("새 세션에 같은 선택");
            assert_eq!(
                trees(&fresh),
                expected,
                "이전 선택의 생성 줄 캐시가 남으면 안 된다"
            );
            assert!(
                !core
                    .set_font_environment(selected_environment)
                    .expect("동일 선택 반복"),
                "같은 선언 반복은 no-op이어야 한다"
            );
            assert_eq!(trees(&core), expected, "반복 선택은 paint를 바꾸면 안 된다");
            core.restore_snapshot_native(snapshot)
                .expect("선택 전 스냅샷 복원");
            assert_eq!(
                trees(&core),
                expected,
                "스냅샷은 현재 선택한 환경의 줄과 기하를 유지해야 한다"
            );
            core.set_document(document.clone());
            assert_eq!(
                trees(&core),
                expected,
                "문서 교체는 현재 선택의 줄을 재구성해야 한다"
            );
            assert_eq!(
                fonts_used(&core),
                expected_fonts,
                "스냅샷 복원과 문서 교체도 현재 선택의 탐색 목록을 보존해야 한다"
            );
            assert_eq!(
                exports(&core),
                original,
                "환경 전환과 복원은 원문 바이트를 보존해야 한다"
            );
            selected_states.push(expected);
        }
        assert_ne!(
            selected_states[0], initial,
            "fallback A는 미선언 측정/paint와 달라야 한다"
        );
        assert_ne!(
            selected_states[0], selected_states[1],
            "같은 ID라도 A/B face 차이는 출력에 반영되어야 한다"
        );
        assert_eq!(
            selected_states[2], initial,
            "None 해제는 원래 출력으로 돌아가야 한다"
        );
    }
}

#[test]
fn unused_fallback_preserves_named_and_embedded_font_selection() {
    for embedded in [false, true] {
        let mut core = DocumentCore::new_empty();
        core.create_blank_document_native().expect("빈 문서 생성");
        core.insert_text_native(0, 0, 0, "가나다")
            .expect("글꼴 선택 대조 문자 입력");
        let mut document = core.document().clone();
        for faces in &mut document.doc_info.font_faces {
            for face in faces {
                face.name = "KoPub돋움체 Light".into();
                face.is_embedded = embedded;
            }
        }
        core.set_document(document);
        let mut declaration =
            json!({"id":"named-control","substitutions":{"KoPub돋움체 Light":"바탕"}});
        core.set_font_environment(Some(
            FontEnvironment::from_json(&declaration.to_string()).expect("명명 face 대조 환경"),
        ))
        .expect("fallback 없는 동일 환경");
        let expected_fonts = if embedded {
            json!(["KoPub돋움체 Light"])
        } else {
            json!(["바탕"])
        };
        assert_eq!(
            fonts_used(&core),
            expected_fonts,
            "명명 face의 기존 치환/내장 보호 결과를 탐색 목록에 반영해야 한다"
        );
        let expected = trees(&core);
        let before = record(&core, "가");
        declaration["fallbackFace"] = json!("HCR Dotum");
        core.set_font_environment(Some(
            FontEnvironment::from_json(&declaration.to_string()).expect("fallback 포함 대조 환경"),
        ))
        .expect("미사용 fallback 추가");
        let after = record(&core, "가");
        assert_eq!(
            fonts_used(&core),
            expected_fonts,
            "미사용 fallback을 명명/내장 face의 탐색 목록에 더하면 안 된다"
        );
        assert_eq!(
            trees(&core),
            expected,
            "명명/내장 face의 출력은 미사용 fallback에 영향받으면 안 된다"
        );
        assert_eq!(
            after, before,
            "명명/내장 face 결정은 기존 규칙을 그대로 사용해야 한다"
        );
        assert_eq!(
            after["document"]["face"], "KoPub돋움체 Light",
            "원문 명명 face를 보존해야 한다"
        );
        assert_eq!(
            after["layoutName"]["normalizedFace"],
            if embedded {
                "KoPub돋움체 Light"
            } else {
                "바탕"
            },
            "내장 face 보호 및 기존 명시적 치환 우선순위"
        );
    }
}

// This scalar contract does not assign page or position expectations to the flag variant.
#[test]
fn fallback_space_metric_obeys_the_use_font_space_flag() {
    for bytes in [HWP, HWPX] {
        for use_font_space in [false, true] {
            let mut core = DocumentCore::from_bytes(bytes).expect("동일 합성 원본 읽기");
            let mut document = core.document().clone();
            for shape in &mut document.doc_info.char_shapes {
                shape.use_font_space = use_font_space;
            }
            core.set_document(document);
            core.set_font_environment(Some(environment(Some("HCR Dotum"))))
                .expect("검증된 HCR fallback 선택");
            let space = record(&core, " ");
            assert_eq!(
                space["layoutName"]["normalizedFace"], "HCR Dotum",
                "공백도 선택한 영문 슬롯 face를 사용해야 한다"
            );
            assert_eq!(
                space["layoutMetric"]["baseAdvanceHwpunit"],
                if use_font_space { 300 } else { 500 },
                "글꼴 공백 플래그는 독립 hmtx의300/1000em과 기존 반각500HU를 구분해야 한다"
            );
            assert_eq!(
                space["layoutMetric"]["widthSource"],
                if use_font_space {
                    "useFontSpace"
                } else {
                    "metricHalfSpace"
                },
                "글꼴 공백 플래그가 실제 측정 갈래를 선택해야 한다"
            );
        }
    }
}

// 결측 charPr는 표시만 복구하며 HWPX 저장의 미등록 참조 검증은 유지한다.
fn implicit_default_exports(
    core: &DocumentCore,
    missing_char_table: bool,
) -> (Vec<u8>, Result<Vec<u8>, String>) {
    if !missing_char_table {
        let (hwp, hwpx) = exports(core);
        return (hwp, Ok(hwpx));
    }
    let hwp = core
        .export_hwp_native()
        .expect("결측 글자모양 원문의 HWP 내보내기");
    let error = core
        .export_hwpx_native()
        .expect_err("결측 charPr 0번 참조의 HWPX 저장을 거절해야 한다");
    let rhwp::HwpError::RenderError(message) = error else {
        panic!("미등록 charPr 참조의 RenderError여야 한다: {error:?}");
    };
    assert_eq!(
        message, "XML 쓰기 실패: 미등록 ID 참조 발견: charPrIDRef: [0]",
        "결측 charPr 입력은 정확히 미등록 0번 글자모양 때문에 저장이 거절되어야 한다"
    );
    (hwp, Err(message))
}

// 공개 환경 선언의 공급 face가 브라우저 준비 목록에 있어야 하는 API 계약이다.
// 합성 결측 입력의 선택·원문 보존만 검사하며 쪽 배치나 시각 일치를 고정하지 않는다.
fn assert_implicit_default_prepares_environment_faces(missing_char_table: bool, source: &str) {
    let mut core = DocumentCore::from_bytes(HWPX).expect("합성 HWPX 기반 읽기");
    let mut document = core.document().clone();
    for faces in &mut document.doc_info.font_faces {
        faces.clear();
    }
    if missing_char_table {
        document.doc_info.char_shapes.clear();
    } else {
        for shape in &mut document.doc_info.char_shapes {
            shape.font_ids[1] = 0;
        }
    }
    core.set_document(document.clone());
    let original = implicit_default_exports(&core, missing_char_table);
    for declaration in [
        json!({"id":"implicit-substitution","substitutions":{(source):"HCR Dotum"}}),
        json!({"id":"implicit-glyphs","substitutions":{},
               "glyphFallbacks":{(source):["HCR Dotum"]}}),
    ] {
        assert!(declaration.get("fallbackFace").is_none());
        core.set_font_environment(Some(
            FontEnvironment::from_json(&declaration.to_string()).expect("기본 face 공급 선언"),
        ))
        .expect("명시적 fallback 없는 환경 선택");
        assert!(
            fonts_used(&core)
                .as_array()
                .expect("준비 글꼴 배열")
                .iter()
                .any(|face| face == "HCR Dotum"),
            "선언한 기본 face 치환 또는 glyph 공급자를 브라우저가 준비해야 한다: {declaration}"
        );
        assert_eq!(
            serde_json::to_value(&core.document().doc_info.font_faces).expect("원문 face JSON"),
            serde_json::to_value(&document.doc_info.font_faces).expect("초기 face JSON"),
            "투영한 기본 face를 원문 언어 슬롯에 기록하면 안 된다"
        );
        assert_eq!(
            serde_json::to_value(&core.document().doc_info.char_shapes)
                .expect("원문 글자모양 JSON"),
            serde_json::to_value(&document.doc_info.char_shapes).expect("초기 글자모양 JSON"),
            "복구한 글자모양을 원문 표에 기록하면 안 된다"
        );
        assert_eq!(
            implicit_default_exports(&core, missing_char_table),
            original,
            "선택은 HWP 바이트와 HWPX 저장 성공 또는 명시적 거절 결과를 보존해야 한다"
        );
    }
    if missing_char_table {
        core.set_font_environment(Some(environment(Some("HCR Batang"))))
            .expect("복구 기본값보다 명시적 fallback 우선 선택");
        assert_eq!(
            fonts_used(&core),
            json!(["HCR Batang"]),
            "명시적 fallback은 미사용 복구 기본값과 그 공급 체인을 대체해야 한다"
        );
        assert_eq!(
            implicit_default_exports(&core, missing_char_table),
            original,
            "명시적 선택도 HWP 바이트와 같은 HWPX 미등록 참조 거절을 보존해야 한다"
        );
        assert!(
            core.document().doc_info.char_shapes.is_empty(),
            "명시적 fallback도 복구한 글자모양을 원문에 기록하면 안 된다"
        );
        assert_eq!(
            serde_json::to_value(&core.document().doc_info.font_faces).expect("원문 face JSON"),
            serde_json::to_value(&document.doc_info.font_faces).expect("초기 face JSON"),
            "명시적 fallback도 원문 언어 슬롯을 보존해야 한다"
        );
    }
}

#[test]
fn missing_char_table_prepares_selected_implicit_default_substitution_and_glyphs() {
    assert_implicit_default_prepares_environment_faces(true, "굴림");
}

#[test]
fn missing_latin_slot_prepares_selected_implicit_default_substitution_and_glyphs() {
    assert_implicit_default_prepares_environment_faces(false, "한컴바탕");
}
