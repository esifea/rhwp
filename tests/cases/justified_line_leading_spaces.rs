//! 독립 한컴 PDF 4쪽에서 줄 첫 공백과 내부 런 첫 공백의 실제 원점을 비교한다.

use rhwp::document_core::DocumentCore;
use rhwp::renderer::render_tree::{RenderNode, RenderNodeType};

fn nodes(node: &RenderNode) -> Vec<&RenderNode> {
    let mut out = vec![node];
    for child in &node.children {
        out.extend(nodes(child));
    }
    out
}

fn compact(text: &str) -> String {
    text.chars().filter(|c| !c.is_whitespace()).collect()
}

fn glyph_origin(node: roxmltree::Node<'_, '_>) -> (f64, f64) {
    if let (Some(x), Some(y)) = (node.attribute("x"), node.attribute("y")) {
        return (
            x.parse().expect("글리프 x 수치"),
            y.parse().expect("글리프 y 수치"),
        );
    }
    let transform = node.attribute("transform").expect("글리프 실제 변환");
    let translation = transform
        .strip_prefix("translate(")
        .and_then(|s| s.split_once(')'))
        .expect("글리프 원점 이동")
        .0;
    let values: Vec<f64> = translation
        .split([',', ' '])
        .filter(|s| !s.is_empty())
        .map(|s| s.parse().expect("원점 이동 수치"))
        .collect();
    assert_eq!(values.len(), 2, "SVG 글리프 원점의 두 축");
    (values[0], values[1])
}

#[test]
fn page_four_excludes_only_the_line_prefix_from_justification() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("samples/task1749/saved_bounds_cumulative_page_break.hwp");
    let core = DocumentCore::from_bytes(&std::fs::read(path).expect("독립 PDF 대응 원본"))
        .expect("원본 열기");
    let tree = core
        .build_page_render_tree(3)
        .expect("검증 범위인 4쪽 배치");
    let svg = core.render_page_svg_native(3).expect("4쪽 공개 SVG 출력");
    let xml = roxmltree::Document::parse(&svg).expect("SVG 구조");
    let glyphs: Vec<_> = xml
        .descendants()
        .filter(|node| node.has_tag_name("text"))
        .map(|node| {
            let (x, y) = glyph_origin(node);
            (x, y, node.text().expect("실제 표시 글리프"))
        })
        .collect();
    // 같은 원본의 2024 PDF를 장치 좌표로 해독한 일곱 줄이다. 다른 쪽은 검사하지 않는다.
    let cases: &[(usize, f64, f64, &[(&str, f64)])] = &[
        (
            47,
            109.44,
            720.0,
            &[
                (
                    "ㅇ (신규위원) 4차 분과회의는 신규위원들 포함해서 신청을 받고 있으며,",
                    114.08,
                ),
                (
                    "신규위원은 OT를 통해 서울청정넷의 주요활동에 대해서 설명함. 15일",
                    151.36,
                ),
                (
                    "부터 신규위원을 카카오워크에 초대할 예정이므로 환대 부탁함.",
                    187.36,
                ),
            ],
        ),
        (
            48,
            108.96,
            706.400_002,
            &[
                (
                    "ㅇ (기존위원) 하반기에 분과가 다른 형태로 전환하기 때문에 기존위원을",
                    224.48,
                ),
                (
                    "대상으로 각 선택활동에 대해 상세하고 세부적인 안내가 필요함.",
                    261.76,
                ),
            ],
        ),
        (
            49,
            107.20,
            706.400_001,
            &[
                (
                    "ㅇ (정책제안) 의제 관심사 위주로 팀빌딩으로 구성할 예정이며, 당일 미참",
                    298.88,
                ),
                (
                    "여자를 위해 구성팀을 공유하고, 구글폼을 통해 신청을 받기로 함.",
                    336.16,
                ),
            ],
        ),
    ];
    for &(para, parenthesis_x, last_x, expected) in cases {
        let mut lines: Vec<_> = nodes(&tree.root).into_iter().filter(|node| matches!(&node.node_type, RenderNodeType::TextLine(line) if line.para_index == Some(para))).collect();
        lines.sort_by(|a, b| a.bbox.y.total_cmp(&b.bbox.y));
        assert_eq!(
            lines.len(),
            expected.len(),
            "문단 {para}의 4쪽 줄 소유와 줄바꿈 보존"
        );
        for (line, &(expected_text, baseline)) in lines.iter().zip(expected) {
            let drawn: String = nodes(line)
                .into_iter()
                .filter_map(|node| match &node.node_type {
                    RenderNodeType::TextRun(run) => Some(run.display_or_text()),
                    _ => None,
                })
                .collect();
            assert_eq!(
                compact(&drawn),
                compact(expected_text),
                "문단 {para}의 실제 줄 내용과 순서"
            );
            let actual: String = glyphs
                .iter()
                .filter(|(_, y, _)| (*y - baseline).abs() <= 0.2)
                .map(|(_, _, text)| *text)
                .collect();
            assert_eq!(
                compact(&actual),
                compact(expected_text),
                "문단 {para}의 실제 글리프와 독립 PDF 줄 기준선"
            );
        }
        let mut first: Vec<_> = glyphs
            .iter()
            .filter(|(_, y, _)| (*y - expected[0].1).abs() <= 0.2)
            .collect();
        first.sort_by(|a, b| a.0.total_cmp(&b.0));
        assert_eq!(
            first.first().expect("첫 표시 글리프").2,
            "ㅇ",
            "문단 표지 글리프 소유"
        );
        assert!(
            (first[0].0 - 77.28).abs() <= 0.2,
            "문단 {para}의 줄 첫 표지 원점 {}, 독립 PDF 77.28",
            first[0].0
        );
        let parenthesis = first
            .iter()
            .find(|glyph| glyph.2 == "(")
            .expect("내부 공백 뒤의 괄호 글리프");
        // 열 글자 이내의 원점 양자화와 PDF 장치 한 칸을 포함한다.
        assert!(
            (parenthesis.0 - parenthesis_x).abs() <= 0.4,
            "문단 {para}의 줄 내부 공백은 계속 양쪽정렬된다: {}, 독립 PDF {parenthesis_x}",
            parenthesis.0
        );
        // 최대 42글자의 누적 원점 반올림과 PDF 장치 한 칸을 포함한다.
        assert!(
            (first.last().expect("마지막 표시 글리프").0 - last_x).abs() <= 1.0,
            "문단 {para}의 줄 끝은 독립 PDF와 같은 줄에 남는다"
        );
    }
}
