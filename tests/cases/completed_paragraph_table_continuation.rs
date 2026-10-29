//! 완성 문단 사이에서 나눈 행은 내용 소유와 원래 물리 띠를 함께 보존한다.

use rhwp::document_core::DocumentCore;
use rhwp::renderer::render_tree::{RenderNode, RenderNodeType};

fn nodes(node: &RenderNode) -> Vec<&RenderNode> {
    let mut out = vec![node];
    for child in &node.children {
        out.extend(nodes(child));
    }
    out
}

fn text(node: &RenderNode) -> String {
    nodes(node)
        .into_iter()
        .filter_map(|node| match &node.node_type {
            RenderNodeType::TextRun(run) => Some(run.display_or_text()),
            _ => None,
        })
        .collect::<String>()
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect()
}

fn table(root: &RenderNode) -> &RenderNode {
    nodes(root)
        .into_iter()
        .find(
            |node| matches!(&node.node_type, RenderNodeType::Table(t) if t.para_index == Some(29)),
        )
        .expect("원본의 전형절차 표")
}

fn cell(table: &RenderNode, row: u16, col: u16) -> &RenderNode {
    table.children.iter().find(|node| matches!(&node.node_type, RenderNodeType::TableCell(c) if c.row == row && c.col == col))
        .expect("PDF와 같은 쪽에 소유된 행과 셀")
}

fn assert_content_inside(cell: &RenderNode) {
    for run in nodes(cell).into_iter().filter(
        |node| matches!(&node.node_type, RenderNodeType::TextRun(r) if !r.text.trim().is_empty()),
    ) {
        assert!(
            run.bbox.x >= cell.bbox.x - 0.2
                && run.bbox.x + run.bbox.width <= cell.bbox.x + cell.bbox.width + 0.2,
            "글자 런의 가로 영역은 소유 셀 안에 있다"
        );
        assert!(
            run.bbox.y >= cell.bbox.y - 0.2
                && run.bbox.y + run.bbox.height <= cell.bbox.y + cell.bbox.height + 0.2,
            "글자 런의 세로 영역은 소유 셀 안에 있다"
        );
    }
}

fn assert_border(table: &RenderNode, cell: &RenderNode, y: f64) {
    assert!(
        nodes(table).into_iter().any(|node| {
            matches!(&node.node_type, RenderNodeType::Line(line) if (line.y1 - line.y2).abs() < 0.01
            && (line.y1 - y).abs() <= 0.1
            && line.x1.min(line.x2) <= cell.bbox.x + 0.1
            && line.x1.max(line.x2) >= cell.bbox.x + cell.bbox.width - 0.1)
        }),
        "소유 셀의 실제 가로 괘선은 y={y}에 있다"
    );
}

#[test]
fn completed_paragraph_row_keeps_ownership_bands_and_following_rows() {
    for sample in [
        "samples/issue1133/page-number-footer15.hwpx",
        "samples/issue1133/page-number-bottom15.hwpx",
    ] {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(sample);
        let core = DocumentCore::from_bytes(&std::fs::read(path).expect("독립 PDF 대응 원본"))
            .expect("원본 열기");
        assert_eq!(core.page_count(), 3, "양쪽 독립 PDF의 세 쪽 소유 유지");
        let pages: Vec<_> = (0..3)
            .map(|p| core.build_page_render_tree(p).expect("실제 쪽 배치"))
            .collect();
        let first = table(&pages[1].root);
        let tail = table(&pages[2].root);
        let left = cell(first, 4, 0);
        let right = cell(first, 4, 1);
        assert_eq!(text(left), "2차면접", "완성된 왼쪽 문단은 2쪽에 남는다");
        assert_eq!(
            text(right),
            "□임원면접",
            "완성된 오른쪽 첫 문단은 2쪽에 남는다"
        );
        let tail_left = cell(tail, 4, 0);
        let tail_right = cell(tail, 4, 1);
        assert!(
            text(tail_left).is_empty(),
            "3쪽 왼쪽 셀은 이미 소비한 문단을 반복하지 않는다"
        );
        assert_eq!(
            text(tail_right),
            "※일정및세부사항은합격자개별통보",
            "3쪽 오른쪽 셀은 남은 문단만 소유한다"
        );
        let whole: String = pages.iter().flat_map(|page| nodes(&page.root)).filter(|node| {
            matches!(&node.node_type, RenderNodeType::Table(t) if t.para_index == Some(29))
        }).map(text).collect();
        assert_eq!(
            whole.matches("2차면접").count(),
            1,
            "왼쪽 문단 누락과 중복 금지"
        );
        assert_eq!(
            whole.matches("□임원면접").count(),
            1,
            "오른쪽 문단 누락과 중복 금지"
        );
        // 독립 PDF의 두 괘선 차이이며 600dpi 장치 한 칸과 양끝 반올림을 포함한다.
        for (table, a, b, expected) in [
            (first, left, right, 39.479_16),
            (tail, tail_left, tail_right, 30.208_33),
        ] {
            assert!(
                (a.bbox.height - expected).abs() <= 0.2,
                "{sample} 실제 물리 띠 {}, 독립 PDF {expected}",
                a.bbox.height
            );
            assert!(
                (a.bbox.y - b.bbox.y).abs() < 0.05 && (a.bbox.height - b.bbox.height).abs() < 0.05,
                "양쪽 셀이 같은 물리 행을 공유한다"
            );
            for c in [a, b] {
                assert_content_inside(c);
                assert_border(table, c, c.bbox.y);
                assert_border(table, c, c.bbox.y + c.bbox.height);
            }
        }
        let mut end = tail_left.bbox.y + tail_left.bbox.height;
        for (row, title, content) in [
            (5, "최종합격", "최종합격자발표및채용시기:4월중"),
            (6, "기타사항", "전형단계별합격자는"),
            (7, "담당자", "02-397-8628"),
        ] {
            let left = cell(tail, row, 0);
            let right = cell(tail, row, 1);
            assert_eq!(text(left), title, "뒤 행 제목과 순서 보존");
            assert!(
                text(right).contains(content),
                "뒤 행의 본문과 마지막 담당자 보존"
            );
            assert!(
                (left.bbox.y - end).abs() < 0.05 && (right.bbox.y - end).abs() < 0.05,
                "이어받은 물리 띠 뒤에 다음 행이 겹침 없이 연결된다"
            );
            end = left.bbox.y + left.bbox.height;
        }
    }
}
