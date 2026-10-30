//! #6869/#6871: 중첩 문단의 쪽번호 방출 상태와 접힌 슬롯의 축을 격리한다.
//! 합성 IR 기반 serializer 계약 검사이며 한컴 유효성 검증용 샘플이 아니다.
#![cfg(not(target_arch = "wasm32"))]

use std::io::Read;

use rhwp::model::control::{Control, PageNumberPos};
use rhwp::model::document::{Document, Section};
use rhwp::model::header_footer::Header;
use rhwp::model::paragraph::{LineSeg, Paragraph};
use rhwp::model::provenance::SourceFormat;
use rhwp::model::table::{Cell, Table};

fn page_num() -> Control {
    Control::PageNumberPos(PageNumberPos::default())
}

fn paragraph(controls: Vec<Control>) -> Paragraph {
    Paragraph {
        char_count: controls.len() as u32 * 8 + 1,
        controls,
        ..Default::default()
    }
}

fn table(child: Paragraph) -> Control {
    Control::Table(Box::new(Table {
        col_count: 1,
        row_count: 1,
        cells: vec![Cell {
            col_span: 1,
            row_span: 1,
            width: 20000,
            paragraphs: vec![child],
            ..Default::default()
        }],
        ..Default::default()
    }))
}

fn plain_child() -> Paragraph {
    Paragraph {
        text: "cell".into(),
        char_count: 5,
        ..Default::default()
    }
}

fn document(paragraphs: Vec<Paragraph>, format: SourceFormat) -> Document {
    let mut doc = Document::default();
    doc.provenance.format = format;
    doc.doc_info.para_shapes.push(Default::default());
    doc.doc_info.char_shapes.push(Default::default());
    doc.sections.push(Section {
        paragraphs,
        ..Default::default()
    });
    doc
}

fn section_xml(doc: &Document) -> String {
    let bytes = rhwp::serializer::hwpx::serialize_hwpx(doc).expect("HWPX 저장");
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut xml = String::new();
    zip.by_name("Contents/section0.xml")
        .unwrap()
        .read_to_string(&mut xml)
        .unwrap();
    xml
}

// descendants 전체를 세면 셀/머리말의 pageNum까지 부모 소유로 오인하므로
// 가장 가까운 문단 조상으로 개수와 위치의 소유권을 한정한다.
fn owned_values(xml: &str) -> Vec<(usize, Vec<u32>)> {
    let parsed = roxmltree::Document::parse(xml).unwrap();
    parsed
        .descendants()
        .filter(|n| n.tag_name().name() == "p")
        .map(|p| {
            let owned: Vec<_> = p
                .descendants()
                .filter(|n| n.ancestors().find(|a| a.tag_name().name() == "p") == Some(p))
                .collect();
            let count = owned
                .iter()
                .filter(|n| n.tag_name().name() == "pageNum")
                .count();
            let positions = owned
                .iter()
                .filter(|n| n.tag_name().name() == "lineseg")
                .map(|n| n.attribute("textpos").unwrap().parse().unwrap())
                .collect();
            (count, positions)
        })
        .collect()
}

fn assert_counts(paragraphs: Vec<Paragraph>, expected: &[usize]) {
    for format in [SourceFormat::Hwp5, SourceFormat::Hwpx] {
        let values = owned_values(&section_xml(&document(paragraphs.clone(), format)));
        let counts: Vec<_> = values.iter().map(|v| v.0).collect();
        assert_eq!(counts, expected, "문단 소유권: {format:?}");
    }
}

#[test]
fn parent_dedup_state_survives_plain_table_child() {
    assert_counts(
        vec![paragraph(vec![
            page_num(),
            table(plain_child()),
            page_num(),
        ])],
        &[1, 0],
    );
}

#[test]
fn child_page_num_does_not_consume_parent_first_page_num() {
    assert_counts(
        vec![paragraph(vec![
            table(paragraph(vec![page_num()])),
            page_num(),
        ])],
        &[1, 1],
    );
}

#[test]
fn sibling_paragraphs_remain_independent() {
    assert_counts(
        vec![
            paragraph(vec![page_num(), page_num()]),
            paragraph(vec![page_num()]),
        ],
        &[1, 1],
    );
}

#[test]
fn empty_child_early_return_restores_parent_state() {
    assert_counts(
        vec![paragraph(vec![
            page_num(),
            table(Paragraph::default()),
            page_num(),
        ])],
        &[1, 0],
    );
}

#[test]
fn two_nested_tables_restore_each_parent_state() {
    let child = paragraph(vec![page_num(), table(plain_child()), page_num()]);
    assert_counts(
        vec![paragraph(vec![page_num(), table(child), page_num()])],
        &[1, 1, 0],
    );
}

#[test]
fn header_paragraphs_do_not_reset_body_dedup_state() {
    let header = Control::Header(Box::new(Header {
        paragraphs: vec![plain_child()],
        ..Default::default()
    }));
    assert_counts(
        vec![paragraph(vec![page_num(), header, page_num()])],
        &[1, 0],
    );
}

fn with_boundary(mut para: Paragraph, start: u32) -> Paragraph {
    para.line_segs = [0, start]
        .into_iter()
        .map(|text_start| LineSeg {
            text_start,
            line_height: 1000,
            text_height: 1000,
            baseline_distance: 800,
            segment_width: 20000,
            tag: 393216,
            ..Default::default()
        })
        .collect();
    para
}

#[test]
fn collapsed_axes_are_local_to_parent_and_child_in_both_formats() {
    // 부모는 중복 쪽번호를 접은 뒤 secPr/colPr 두 슬롯을 앞에 보강한다.
    // 자식에는 정의를 보강하지 않는다. 각 문단의 마지막 표 앞 슬롯만 센다.
    let child = with_boundary(
        paragraph(vec![page_num(), page_num(), table(plain_child())]),
        16,
    );
    let parent = with_boundary(
        paragraph(vec![
            page_num(),
            table(child),
            page_num(),
            table(plain_child()),
        ]),
        24,
    );
    for format in [SourceFormat::Hwp5, SourceFormat::Hwpx] {
        let xml = section_xml(&document(vec![parent.clone()], format));
        let parsed = roxmltree::Document::parse(&xml).expect("section XML 파싱");
        let owned_slots: Vec<Vec<_>> = parsed
            .descendants()
            .filter(|n| n.has_tag_name("p"))
            .map(|p| {
                p.descendants()
                    .filter(|n| n.ancestors().find(|a| a.has_tag_name("p")) == Some(p))
                    .map(|n| n.tag_name().name())
                    .filter(|name| matches!(*name, "secPr" | "colPr" | "pageNum" | "tbl"))
                    .collect()
            })
            .collect();
        assert_eq!(
            owned_slots,
            vec![
                vec!["secPr", "colPr", "pageNum", "tbl", "tbl"],
                vec!["pageNum", "tbl"],
                vec![],
                vec![],
            ],
            "정의 보강과 중복 제거는 소유 문단 안에서만 적용한다: {format:?}"
        );
        let last_table_start =
            |slots: &[&str]| slots.iter().rposition(|name| *name == "tbl").unwrap() as u32 * 8;
        let values = owned_values(&xml);
        assert_eq!(values.len(), 4);
        assert_eq!(
            values[0],
            (1, vec![0, last_table_start(&owned_slots[0])]),
            "부모 축은 실제 방출한 자기 슬롯만 센다: {format:?}"
        );
        assert_eq!(
            values[1],
            (1, vec![0, last_table_start(&owned_slots[1])]),
            "자식 축은 부모 정의와 다른 문단의 제거 슬롯을 세지 않는다: {format:?}"
        );
        assert_eq!(values[2].0, 0);
        assert_eq!(values[3].0, 0);
    }
}
