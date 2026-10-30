//! 관계형: 저장 줄의 텍스트 축 경계를 공개 직렬화와 재파싱으로 보존한다.
//! 기존 source 검사의 이관이며 픽셀 좌표나 시각 일치율을 고정하지 않는다.

use rhwp::model::document::Section;
use rhwp::model::paragraph::{CharShapeRef, LineSeg, Paragraph};
use rhwp::parser::body_text::parse_body_text_section;
use rhwp::serializer::body_text::serialize_section;

/// [#4677] PARA_TEXT 밖을 가리키는 lineseg 는 파일에 나가지 않는다.
///
/// 색인 표시(`idxm`)처럼 HWP5 저장에서 떨어지는 컨트롤이 있으면 문단이 8 유닛 짧아지는데,
/// 원본 HWPX 의 lineseg 는 그 컨트롤을 센 `textpos` 를 그대로 들고 있다. 그 값을 그대로
/// 쓰면 한글 2022 가 본문 전체를 버리고 빈 1쪽으로 연다.
#[test]
fn test_out_of_range_line_segs_are_not_serialized() {
    let para = Paragraph {
        char_count: 3,
        text: "AB".to_string(),
        char_offsets: vec![0, 1],
        char_shapes: vec![CharShapeRef {
            start_pos: 0,
            char_shape_id: 0,
        }],
        line_segs: vec![
            LineSeg {
                text_start: 0,
                line_height: 500,
                ..Default::default()
            },
            // 떨어져 나간 8 유닛 컨트롤을 센 잔재: "AB" + 문단끝 = 3 유닛보다 멀리 나간다.
            // (경계값 `text_start == char_count` 는 한컴도 쓰는 정상 값이라 남긴다.)
            LineSeg {
                text_start: 10,
                line_height: 500,
                ..Default::default()
            },
        ],
        ..Default::default()
    };

    let section = Section {
        paragraphs: vec![para],
        raw_stream: None,
        raw_provenance: None,
        ..Default::default()
    };

    let bytes = serialize_section(&section);
    let parsed = parse_body_text_section(&bytes).unwrap();

    assert_eq!(
        parsed.paragraphs[0].line_segs.len(),
        1,
        "범위 밖 lineseg 는 레코드에서 제외된다"
    );
    assert_eq!(parsed.paragraphs[0].line_segs[0].text_start, 0);
}

/// [#4677] 끝 위치를 가리키는 lineseg 는 남는다: 한글이 직접 쓰는 값이다.
///
/// 빈 문단은 `char_count=1` 에 줄 `[0, 1]` 로 저장되는 일이 흔하다(저장소 샘플 5건에
/// 40개 문단). 경계를 `>=` 로 잡으면 그 둘째 줄까지 잘려 평범한 문서를 편집·저장할
/// 때마다 조판 정보가 사라진다: 범위 밖 판정은 끝을 **넘어선** 값에만 걸어야 한다.
#[test]
fn test_line_seg_at_text_end_is_kept() {
    let para = Paragraph {
        char_count: 1,
        line_segs: vec![
            LineSeg {
                text_start: 0,
                line_height: 500,
                ..Default::default()
            },
            // 끝 위치(= char_count)를 가리키는 줄: 한글 원본에 실재한다.
            LineSeg {
                text_start: 1,
                line_height: 500,
                ..Default::default()
            },
        ],
        ..Default::default()
    };

    let section = Section {
        paragraphs: vec![para],
        raw_stream: None,
        raw_provenance: None,
        ..Default::default()
    };

    let bytes = serialize_section(&section);
    let parsed = parse_body_text_section(&bytes).unwrap();

    assert_eq!(
        parsed.paragraphs[0].line_segs.len(),
        2,
        "끝 위치를 가리키는 줄은 범위 밖이 아니다"
    );
}
