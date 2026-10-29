//! 자동 쪽번호의 실제 글꼴과 기준선을 같은 원본의 한컴 PDF와 비교한다.

use rhwp::document_core::DocumentCore;
use rhwp::renderer::render_tree::{RenderNode, RenderNodeType, TextRunNode};

const SAMPLE: &str = "samples/hwpx/opengov/36385445_결재문서본문_화재발생종합보고서(제2026-189호, 2026. 5. 14.).hwpx";

fn open() -> DocumentCore {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(SAMPLE);
    DocumentCore::from_bytes(&std::fs::read(path).expect("독립 PDF 대응 원본"))
        .expect("원본 문서 열기")
}

fn number_runs<'a>(node: &'a RenderNode, out: &mut Vec<&'a TextRunNode>) {
    if let RenderNodeType::TextRun(run) = &node.node_type {
        out.push(run);
    }
    for child in &node.children {
        number_runs(child, out);
    }
}

#[test]
fn automatic_number_uses_normal_latin_family_at_regular_ten_points() {
    let core = open();
    for page in 0..2 {
        let tree = core.build_page_render_tree(page).expect("실제 쪽 배치");
        let footer = tree
            .root
            .children
            .iter()
            .find(|node| matches!(node.node_type, RenderNodeType::Footer))
            .expect("자동 쪽번호 꼬리말");
        let mut runs = Vec::new();
        number_runs(footer, &mut runs);
        assert_eq!(runs.len(), 1, "꼬리말은 자동 쪽번호 한 런만 소유한다");
        let run = runs[0];
        assert_eq!(
            run.text.split_whitespace().collect::<String>(),
            format!("-{}-", page + 1),
            "쪽번호 내용 보존"
        );
        // 독립 원본의 Normal Latin은 HY신명조이며 제어 런은 굴림체 12pt다.
        assert_eq!(
            run.style.font_family.split(',').next(),
            Some("HY신명조"),
            "자동 쪽번호의 실제 Latin 글꼴"
        );
        assert!(
            (run.style.font_size - 40.0 / 3.0).abs() < 1e-6,
            "자동 쪽번호의 명목 크기는 10pt다"
        );
        assert!(
            !run.style.bold && !run.style.italic,
            "자동 쪽번호는 보통체다"
        );
        let svg = core.render_page_svg_native(page).expect("공개 SVG 출력");
        let xml = roxmltree::Document::parse(&svg).expect("SVG 구조");
        let glyphs: Vec<_> = xml
            .descendants()
            .filter(|node| {
                node.has_tag_name("text")
                    && node
                        .attribute("y")
                        .and_then(|v| v.parse::<f64>().ok())
                        .is_some_and(|y| y > 1_050.0)
            })
            .collect();
        assert_eq!(
            glyphs.len(),
            3,
            "실제 꼬리말은 대시와 숫자를 한 번씩 그린다"
        );
        for glyph in glyphs {
            assert!(
                glyph
                    .attribute("font-family")
                    .expect("그리기 글꼴")
                    .starts_with("'HY신명조'"),
                "실제 SVG도 같은 Latin 글꼴을 쓴다"
            );
            let size: f64 = glyph
                .attribute("font-size")
                .expect("그리기 크기")
                .parse()
                .expect("크기 수치");
            assert!(
                (size - 40.0 / 3.0).abs() < 1e-6,
                "실제 SVG의 명목 크기는 10pt다"
            );
            assert_eq!(
                glyph.attribute("fill"),
                Some("#000000"),
                "실제 쪽번호는 검정이다"
            );
        }
    }
}

#[test]
fn automatic_number_uses_the_independent_pdf_device_baseline() {
    let core = open();
    for page in 0..2 {
        let svg = core.render_page_svg_native(page).expect("공개 SVG 출력");
        let xml = roxmltree::Document::parse(&svg).expect("SVG 구조");
        let mut drawn = String::new();
        for glyph in xml.descendants().filter(|node| node.has_tag_name("text")) {
            let Some(y) = glyph.attribute("y").and_then(|v| v.parse::<f64>().ok()) else {
                continue;
            };
            if y <= 1_050.0 {
                continue;
            }
            drawn.push_str(glyph.text().expect("실제 쪽번호 글자"));
            // 양쪽 PDF의 Tm y6767을 600dpi 장치 좌표에서 96dpi로 환산한다.
            assert!(
                (y - 1_082.72).abs() <= 0.2,
                "{}쪽 쪽번호 기준선 {y}, 독립 PDF 1082.72",
                page + 1
            );
        }
        assert_eq!(
            drawn,
            format!("-{}-", page + 1),
            "기준선 검사는 실제 쪽번호 전체를 포함한다"
        );
    }
}
