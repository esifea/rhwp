//! Memo bodies belong to their fields, including fields inside merged/nested cells.
use rhwp::model::control::{Control, Field, FieldType};
use rhwp::model::document::{Document, Section};
use rhwp::model::paragraph::{CharShapeRef, FieldRange, LineSeg, Paragraph};
use rhwp::model::table::{Cell, Table};
use rhwp::parser::body_text::parse_body_text_section;
use rhwp::serializer::body_text::serialize_section;

fn paragraph(text: &str) -> Paragraph {
    Paragraph {
        text: text.into(),
        char_shapes: vec![CharShapeRef {
            start_pos: 0,
            char_shape_id: 0,
        }],
        ..Default::default()
    }
}

fn annotated(text: &str, index: u32, notes: &[&str]) -> Paragraph {
    let mut para = paragraph(text);
    para.controls.push(Control::Field(Field {
        field_type: FieldType::Memo,
        ctrl_id: rhwp::parser::tags::FIELD_MEMO,
        command: format!(r"MEMO/65535/{index}/123/456/Synthetic Author/\;;"),
        field_id: 100 + index,
        memo_index: index,
        memo_paragraphs: notes.iter().map(|text| paragraph(text)).collect(),
        ..Default::default()
    }));
    para.field_ranges.push(FieldRange {
        start_char_idx: 0,
        end_char_idx: text.chars().count(),
        control_idx: 0,
        ..Default::default()
    });
    para
}

fn field(para: &Paragraph) -> &Field {
    para.controls
        .iter()
        .find_map(|control| match control {
            Control::Field(field) => Some(field),
            _ => None,
        })
        .expect("memo owner")
}

fn assert_memo(para: &Paragraph, index: u32, expected: &[&str]) {
    let memo = field(para);
    assert_eq!(
        memo.memo_paragraphs
            .iter()
            .map(|para| para.text.as_str())
            .collect::<Vec<_>>(),
        expected
    );
    assert_eq!(memo.memo_index, index);
    assert_eq!(memo.field_id, 100 + index);
    assert_eq!(
        memo.command,
        format!(r"MEMO/65535/{index}/123/456/Synthetic Author/\;;")
    );
    assert_eq!(para.field_ranges.len(), 1, "memo anchor range");
    assert_eq!(para.field_ranges[0].start_char_idx, 0);
    assert_eq!(para.field_ranges[0].end_char_idx, para.text.chars().count());
}

fn roundtrip(section: &Section) -> Section {
    parse_body_text_section(&serialize_section(section)).expect("parse generated HWP5 records")
}

#[test]
fn issue_1391_memo_hwp5_top_level_keeps_body_count_and_multiple_empty_notes() {
    let section = Section {
        paragraphs: vec![
            annotated("본문 대상", 7, &["첫 메모", "", "Third memo"]),
            paragraph("마지막 본문"),
        ],
        ..Default::default()
    };
    let output = roundtrip(&section);

    assert_memo(&output.paragraphs[0], 7, &["첫 메모", "", "Third memo"]);

    assert_eq!(
        output.paragraphs.len(),
        2,
        "memo container is not a body paragraph"
    );
    assert_eq!(output.paragraphs[1].text, "마지막 본문");

    let again = roundtrip(&output);
    assert_eq!(again.paragraphs.len(), 2);

    assert_memo(&again.paragraphs[0], 7, &["첫 메모", "", "Third memo"]);
}

fn table(paragraphs: Vec<Paragraph>, merged: bool) -> Table {
    let mut table = Table {
        row_count: 1,
        col_count: if merged { 2 } else { 1 },
        cells: vec![Cell {
            row: 0,
            col: 0,
            row_span: 1,
            col_span: if merged { 2 } else { 1 },
            width: 14000,
            height: 6000,
            paragraphs,
            ..Default::default()
        }],
        ..Default::default()
    };
    table.common.width = 14000;
    table.common.height = 6000;
    table.rebuild_grid();
    table
}

#[test]
fn issue_1391_memo_hwp5_nested_merged_table_keeps_owners() {
    let inner = table(
        vec![annotated("안쪽 대상", 12, &["안쪽 메모", "둘째 줄"])],
        false,
    );
    let mut nested = paragraph("");
    nested.controls.push(Control::Table(Box::new(inner)));

    let outer = table(
        vec![annotated("병합 대상", 11, &["병합 메모"]), nested],
        true,
    );
    let mut body = paragraph("");
    body.controls.push(Control::Table(Box::new(outer)));

    let section = Section {
        paragraphs: vec![body, paragraph("표 뒤 본문")],
        ..Default::default()
    };
    let output = roundtrip(&section);
    let Control::Table(outer) = &output.paragraphs[0].controls[0] else {
        panic!("outer table");
    };

    assert_eq!(outer.cells[0].col_span, 2);
    assert_memo(&outer.cells[0].paragraphs[0], 11, &["병합 메모"]);
    let Control::Table(inner) = &outer.cells[0].paragraphs[1].controls[0] else {
        panic!("inner table");
    };
    assert_memo(&inner.cells[0].paragraphs[0], 12, &["안쪽 메모", "둘째 줄"]);
    assert_eq!(output.paragraphs.len(), 2);
    assert_eq!(output.paragraphs[1].text, "표 뒤 본문");
}

#[test]
fn issue_1391_memo_hwp5_to_hwpx_keeps_native_memo_body_and_metadata() {
    let section = Section {
        paragraphs: vec![annotated(
            "메모 위치",
            4,
            &["한글 주석", "Second annotation", ""],
        )],
        ..Default::default()
    };
    let output = roundtrip(&section);
    let mut document = Document::default();
    document.doc_info.char_shapes = vec![Default::default()];
    document.doc_info.para_shapes = vec![Default::default()];
    document.sections.push(output);

    let hwpx = rhwp::serializer::serialize_hwpx(&document).expect("serialize HWPX");
    let parsed = rhwp::parser::hwpx::parse_hwpx(&hwpx).expect("parse HWPX");

    assert_memo(
        &parsed.sections[0].paragraphs[0],
        4,
        &["한글 주석", "Second annotation", ""],
    );
    assert_eq!(parsed.sections[0].paragraphs.len(), 1);

    let memo = field(&parsed.sections[0].paragraphs[0]);
    assert_eq!(memo.field_type, FieldType::Memo);
    assert!(memo
        .raw_parameters_xml
        .as_deref()
        .unwrap()
        .contains("Synthetic Author"));
}

#[test]
fn issue_1391_memo_hwp5_nonmemo_field_is_unchanged() {
    let mut para = paragraph("일반 필드");
    para.controls.push(Control::Field(Field {
        field_type: FieldType::CrossRef,
        ctrl_id: rhwp::parser::tags::FIELD_CROSSREF,
        command: "SOMETHING/ELSE".into(),
        field_id: 88,
        ..Default::default()
    }));

    let section = Section {
        paragraphs: vec![para],
        ..Default::default()
    };
    let output = roundtrip(&section);
    assert_eq!(output.paragraphs.len(), 1);
    assert_eq!(field(&output.paragraphs[0]).field_type, FieldType::CrossRef);
    assert_eq!(field(&output.paragraphs[0]).command, "SOMETHING/ELSE");
    assert!(field(&output.paragraphs[0]).memo_paragraphs.is_empty());
}

#[test]
fn issue_1391_memo_hwp5_document_final_tail_links_to_earlier_section() {
    let mut document = Document::default();
    document.doc_info.char_shapes = vec![Default::default()];
    document.doc_info.para_shapes = vec![Default::default()];
    document.sections = vec![
        Section {
            paragraphs: vec![annotated("앞 구역 대상", 21, &["앞 구역 주석"])],
            ..Default::default()
        },
        Section {
            paragraphs: vec![annotated("뒤 구역 대상", 22, &["뒤 구역 주석", ""])],
            ..Default::default()
        },
    ];

    let bytes = rhwp::serializer::serialize_hwp(&document).expect("write multi-section HWP");
    let parsed = rhwp::parse_document(&bytes).expect("parse multi-section HWP");
    assert_eq!(parsed.sections.len(), 2);
    assert_eq!(parsed.sections[0].paragraphs.len(), 1);
    assert_eq!(parsed.sections[1].paragraphs.len(), 1);
    assert_memo(&parsed.sections[0].paragraphs[0], 21, &["앞 구역 주석"]);
    assert_memo(&parsed.sections[1].paragraphs[0], 22, &["뒤 구역 주석", ""]);

    let first_records =
        rhwp::parser::record::Record::read_all(parsed.sections[0].raw_stream.as_deref().unwrap())
            .unwrap();
    let final_records =
        rhwp::parser::record::Record::read_all(parsed.sections[1].raw_stream.as_deref().unwrap())
            .unwrap();
    assert_eq!(
        first_records
            .iter()
            .filter(|r| r.tag_id == rhwp::parser::tags::HWPTAG_MEMO_LIST)
            .count(),
        0
    );
    assert_eq!(
        final_records
            .iter()
            .filter(|r| r.tag_id == rhwp::parser::tags::HWPTAG_MEMO_LIST)
            .count(),
        2
    );

    let mut edited = parsed;
    edited.sections[0].paragraphs[0].text = "앞 구역 변경".into();

    let saved = rhwp::serializer::serialize_hwp(&edited).expect("save native HWP edit");
    let reparsed = rhwp::parse_document(&saved).expect("parse native edit");
    assert_memo(&reparsed.sections[0].paragraphs[0], 21, &["앞 구역 주석"]);
    assert_memo(
        &reparsed.sections[1].paragraphs[0],
        22,
        &["뒤 구역 주석", ""],
    );
    assert_eq!(reparsed.sections[0].paragraphs[0].text, "앞 구역 변경");
}

#[test]
fn issue_1391_memo_hwp5_empty_list_and_vertical_direction_roundtrip() {
    let mut vertical = annotated("세로 대상", 31, &["세로 주석"]);
    let Control::Field(memo) = &mut vertical.controls[0] else {
        panic!("memo");
    };
    memo.memo_text_direction = Some("VERTICAL".into());

    let section = Section {
        paragraphs: vec![vertical, annotated("빈 주석", 32, &[])],
        ..Default::default()
    };
    let parsed = roundtrip(&section);
    assert_eq!(parsed.paragraphs.len(), 2);
    assert_memo(&parsed.paragraphs[0], 31, &["세로 주석"]);
    assert_eq!(
        field(&parsed.paragraphs[0]).memo_text_direction.as_deref(),
        Some("VERTICAL")
    );
    assert_memo(&parsed.paragraphs[1], 32, &[]);
}

#[test]
fn issue_1391_memo_hwp5_incomplete_list_is_not_silently_discarded() {
    let section = Section {
        paragraphs: vec![annotated("주석 대상", 41, &["유지할 주석"])],
        ..Default::default()
    };
    let mut records = rhwp::parser::record::Record::read_all(&serialize_section(&section)).unwrap();
    let header = records
        .iter_mut()
        .find(|r| r.tag_id == rhwp::parser::tags::HWPTAG_LIST_HEADER && r.level == 1)
        .unwrap();
    header.data[..4].copy_from_slice(&2_u32.to_le_bytes());

    let malformed = rhwp::serializer::record_writer::write_records(&records);
    assert!(
        parse_body_text_section(&malformed).is_err(),
        "declared memo paragraph cannot disappear"
    );
}

#[test]
fn issue_1391_memo_hwp5_extra_paragraph_is_not_silently_discarded() {
    let section = Section {
        paragraphs: vec![annotated("메모 대상", 51, &["누락 금지"])],
        ..Default::default()
    };
    let mut records = rhwp::parser::record::Record::read_all(&serialize_section(&section)).unwrap();
    let header = records
        .iter_mut()
        .find(|r| r.tag_id == rhwp::parser::tags::HWPTAG_LIST_HEADER && r.level == 1)
        .unwrap();
    header.data[..4].copy_from_slice(&0_u32.to_le_bytes());

    let malformed = rhwp::serializer::record_writer::write_records(&records);
    assert!(
        parse_body_text_section(&malformed).is_err(),
        "undeclared memo paragraph cannot disappear"
    );
}

#[test]
fn issue_1391_memo_hwp5_wrong_list_level_is_rejected() {
    let section = Section {
        paragraphs: vec![annotated("메모 대상", 52, &["올바른 소유권"])],
        ..Default::default()
    };
    let mut records = rhwp::parser::record::Record::read_all(&serialize_section(&section)).unwrap();
    let memo = records
        .iter_mut()
        .find(|r| r.tag_id == rhwp::parser::tags::HWPTAG_MEMO_LIST)
        .unwrap();
    memo.level = 2;

    let malformed = rhwp::serializer::record_writer::write_records(&records);
    assert!(
        parse_body_text_section(&malformed).is_err(),
        "memo tail must have native section ownership"
    );
}

#[test]
fn issue_1391_memo_hwp5_begin_marker_owns_arbitrary_command_not_unknown_bit15() {
    let mut unrelated = paragraph("Unknown field");
    unrelated.controls.push(Control::Field(Field {
        field_type: FieldType::Unknown,
        ctrl_id: rhwp::parser::tags::FIELD_UNKNOWN,
        properties: 0x8000,
        ..Default::default()
    }));

    let mut owner = annotated("진짜 메모", 0, &["명확한 메모 소유권"]);
    let Control::Field(memo) = &mut owner.controls[0] else {
        panic!("memo owner");
    };
    memo.command = "Arbitrary memo command".into();

    let section = Section {
        paragraphs: vec![unrelated, owner],
        ..Default::default()
    };
    let parsed = roundtrip(&section);
    assert_eq!(parsed.paragraphs.len(), 2);
    assert_eq!(field(&parsed.paragraphs[0]).field_type, FieldType::Unknown);
    assert!(field(&parsed.paragraphs[0]).memo_paragraphs.is_empty());
    assert_eq!(field(&parsed.paragraphs[1]).field_type, FieldType::Memo);
    assert_eq!(
        field(&parsed.paragraphs[1]).command,
        "Arbitrary memo command"
    );
    assert_eq!(
        field(&parsed.paragraphs[1]).memo_paragraphs[0].text,
        "명확한 메모 소유권"
    );

    let mut document = Document::default();
    document.doc_info.char_shapes = vec![Default::default()];
    document.doc_info.para_shapes = vec![Default::default()];
    document.sections.push(parsed);

    let native = rhwp::serializer::serialize_hwp(&document).expect("serialize anonymous memo HWP");
    let reloaded = rhwp::parse_document(&native).expect("reload anonymous memo HWP");
    assert_eq!(
        field(&reloaded.sections[0].paragraphs[1]).memo_paragraphs[0].text,
        "명확한 메모 소유권"
    );
}

fn native_document(sections: Vec<Section>) -> Document {
    let mut document = Document::default();
    document.doc_info.char_shapes = vec![Default::default()];
    document.doc_info.para_shapes = vec![Default::default()];
    document.sections = sections;
    document
}

fn native_memo_metadata_fixture(
    header_suffix: &[u8],
    line_segs: &[LineSeg],
    nested: bool,
) -> (Vec<u8>, Vec<u8>) {
    use rhwp::parser::record::Record;
    use rhwp::parser::tags;

    let owner = annotated("Native memo owner", 61, &["Native memo"]);
    let body = if nested {
        let mut outer = paragraph("Table owner");
        outer
            .controls
            .push(Control::Table(Box::new(table(vec![owner], true))));
        outer
    } else {
        owner
    };
    let section = Section {
        paragraphs: vec![body],
        ..Default::default()
    };
    let original =
        rhwp::serializer::serialize_hwp(&native_document(vec![section.clone()])).unwrap();
    let mut records = Record::read_all(&serialize_section(&section)).unwrap();
    let memo = records
        .iter()
        .position(|record| record.tag_id == tags::HWPTAG_MEMO_LIST)
        .unwrap();
    let header = records
        .iter()
        .enumerate()
        .skip(memo + 1)
        .find(|(_, record)| record.tag_id == tags::HWPTAG_PARA_HEADER && record.level == 1)
        .map(|(index, _)| index)
        .unwrap();
    let mut extra = Vec::new();
    extra.extend_from_slice(&1_u16.to_le_bytes()); // one character shape
    extra.extend_from_slice(&0_u16.to_le_bytes()); // no range tags
    extra.extend_from_slice(&(line_segs.len() as u16).to_le_bytes());
    extra.extend_from_slice(header_suffix);

    records[header].data.truncate(12);
    records[header].data.extend_from_slice(&extra);
    records[header].size = records[header].data.len() as u32;

    if !line_segs.is_empty() {
        let mut rows = Vec::new();
        for seg in line_segs {
            rows.extend_from_slice(&seg.text_start.to_le_bytes());
            for value in [
                seg.vertical_pos,
                seg.line_height,
                seg.text_height,
                seg.baseline_distance,
                seg.line_spacing,
                seg.column_start,
                seg.segment_width,
            ] {
                rows.extend_from_slice(&value.to_le_bytes());
            }
            rows.extend_from_slice(&seg.tag.to_le_bytes());
        }

        let end = records
            .iter()
            .enumerate()
            .skip(header + 1)
            .find(|(_, record)| record.level <= 1)
            .map_or(records.len(), |(index, _)| index);
        records.insert(
            end,
            Record {
                tag_id: tags::HWPTAG_PARA_LINE_SEG,
                level: 2,
                size: rows.len() as u32,
                data: rows,
            },
        );
    }

    let stream = rhwp::serializer::record_writer::write_records(&records);
    (replace_section_stream(&original, &stream), extra)
}

fn forced_rebuild(document: &Document) -> Document {
    let mut edited = document.clone();
    edited.doc_info.raw_stream_dirty = true;

    for section in &mut edited.sections {
        section.raw_stream = None;
    }

    rhwp::parse_document(&rhwp::serializer::serialize_hwp(&edited).unwrap()).unwrap()
}

#[test]
fn issue_1391_memo_hwp5_forced_rebuild_preserves_native_header_tails() {
    for suffix in [
        vec![0x78, 0x56, 0x34, 0x12],
        vec![0x78, 0x56, 0x34, 0x12, 0x9a, 0xbc],
    ] {
        let (bytes, expected) = native_memo_metadata_fixture(&suffix, &[], false);
        let native = rhwp::parse_document(&bytes).unwrap();
        assert_eq!(
            field(&native.sections[0].paragraphs[0]).memo_paragraphs[0].raw_header_extra,
            expected,
            "independent native record fixture"
        );

        let rebuilt = forced_rebuild(&native);
        let memo = &field(&rebuilt.sections[0].paragraphs[0]).memo_paragraphs[0];
        assert_eq!(memo.text, "Native memo");
        assert_eq!(memo.raw_header_extra, expected, "native header tail bytes");
        assert!(memo.line_segs.is_empty());

        let again = forced_rebuild(&rebuilt);
        assert_eq!(
            field(&again.sections[0].paragraphs[0]).memo_paragraphs[0].raw_header_extra,
            expected,
            "repeated forced rebuild"
        );
    }
}

#[test]
fn issue_1391_memo_hwp5_forced_rebuild_preserves_nested_native_line_segments() {
    let row = LineSeg {
        text_start: 0,
        vertical_pos: 0,
        line_height: 900,
        text_height: 900,
        baseline_distance: 765,
        line_spacing: 540,
        column_start: 0,
        segment_width: 48188,
        tag: 0x60000,
    };
    let rows = vec![
        row.clone(),
        LineSeg {
            text_start: 7,
            vertical_pos: 1440,
            ..row
        },
    ];
    let (bytes, expected_header) =
        native_memo_metadata_fixture(&[0x78, 0x56, 0x34, 0x12, 0x9a, 0xbc], &rows, true);
    let native = rhwp::parse_document(&bytes).unwrap();
    let Control::Table(original_table) = &native.sections[0].paragraphs[0].controls[0] else {
        panic!("native table owner");
    };

    let original = &field(&original_table.cells[0].paragraphs[0]).memo_paragraphs[0];
    assert_eq!(original.raw_header_extra, expected_header);
    assert_eq!(original.line_segs.len(), 2);

    let rebuilt = forced_rebuild(&native);
    let Control::Table(rebuilt_table) = &rebuilt.sections[0].paragraphs[0].controls[0] else {
        panic!("rebuilt table owner");
    };

    let memo = &field(&rebuilt_table.cells[0].paragraphs[0]).memo_paragraphs[0];
    assert_eq!(memo.text, "Native memo");
    assert_eq!(memo.raw_header_extra, expected_header);
    assert_eq!(
        serde_json::to_value(&memo.line_segs).unwrap(),
        serde_json::to_value(&rows).unwrap(),
        "all native row metrics survive"
    );
}

#[test]
fn issue_1391_memo_hwp5_last_memo_deletion_removes_unchanged_final_tail() {
    let document = native_document(vec![
        Section {
            paragraphs: vec![annotated("Memo owner", 7, &["Remove only when requested"])],
            ..Default::default()
        },
        Section {
            paragraphs: vec![paragraph("Untouched final body")],
            ..Default::default()
        },
    ]);
    let original = rhwp::serializer::serialize_hwp(&document).unwrap();
    let mut edited = rhwp::parse_document(&original).unwrap();
    edited.sections[0].paragraphs[0].controls.clear();
    edited.sections[0].paragraphs[0].field_ranges.clear();

    let saved = rhwp::serializer::serialize_hwp(&edited).unwrap();
    let reparsed = rhwp::parse_document(&saved).expect("last memo deletion must remain parseable");
    assert_eq!(reparsed.sections.len(), 2);
    assert_eq!(reparsed.sections[1].paragraphs.len(), 1);
    assert_eq!(
        reparsed.sections[1].paragraphs[0].text,
        "Untouched final body"
    );
    assert!(reparsed.sections[0].paragraphs[0].controls.is_empty());

    for section in &reparsed.sections {
        let records =
            rhwp::parser::record::Record::read_all(section.raw_stream.as_deref().unwrap()).unwrap();
        assert!(records
            .iter()
            .all(|record| record.tag_id != rhwp::parser::tags::HWPTAG_MEMO_LIST));
    }
}

#[test]
fn issue_1391_memo_hwp5_native_blank_final_body_keeps_style_and_line_position() {
    let mut blank = paragraph("");
    blank.has_para_text = false;
    blank.style_id = 9;
    blank.line_segs = vec![LineSeg {
        text_start: 0,
        vertical_pos: 1200,
        line_height: 500,
        text_height: 500,
        baseline_distance: 400,
        line_spacing: 600,
        ..Default::default()
    }];
    let section = Section {
        paragraphs: vec![annotated("Memo owner", 7, &["Preserved annotation"]), blank],
        ..Default::default()
    };
    let records = rhwp::parser::record::Record::read_all(&serialize_section(&section)).unwrap();
    assert_eq!(
        records
            .iter()
            .filter(|record| {
                record.tag_id == rhwp::parser::tags::HWPTAG_PARA_HEADER && record.level == 0
            })
            .count(),
        2,
        "memo tail must attach to the two real body paragraphs"
    );

    let mut parsed = roundtrip(&section);
    assert_eq!(parsed.paragraphs.len(), 2);
    assert_eq!(parsed.paragraphs[1].style_id, 9);
    assert_eq!(parsed.paragraphs[1].line_segs[0].vertical_pos, 1200);
    assert_eq!(parsed.paragraphs[1].line_segs[0].line_spacing, 600);

    let Control::Field(memo) = &mut parsed.paragraphs[0].controls[0] else {
        panic!("memo owner");
    };
    memo.memo_paragraphs[0].text = "Updated annotation".into();

    let updated = roundtrip(&parsed);
    assert_eq!(updated.paragraphs.len(), 2);
    assert_eq!(updated.paragraphs[1].style_id, 9);
    assert_eq!(updated.paragraphs[1].line_segs[0].vertical_pos, 1200);

    assert_memo(&updated.paragraphs[0], 7, &["Updated annotation"]);
}

fn replace_section_stream(original: &[u8], section: &[u8]) -> Vec<u8> {
    use std::io::{Cursor, Read, Write};
    let mut container = cfb::CompoundFile::open(Cursor::new(original.to_vec())).unwrap();
    let mut header = Vec::new();
    container
        .open_stream("/FileHeader")
        .unwrap()
        .read_to_end(&mut header)
        .unwrap();

    let payload = if header[36] & 1 != 0 {
        let mut encoder =
            flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(section).unwrap();
        encoder.finish().unwrap()
    } else {
        section.to_vec()
    };
    container
        .create_stream("/BodyText/Section0")
        .unwrap()
        .write_all(&payload)
        .unwrap();
    container.into_inner().into_inner()
}

fn lenient_fixture(data: &[u8]) -> Vec<u8> {
    let mut result = data.to_vec();
    assert_eq!(u16::from_le_bytes(result[30..32].try_into().unwrap()), 9);
    assert_eq!(u32::from_le_bytes(result[44..48].try_into().unwrap()), 1);

    let fat = 512 + u32::from_le_bytes(result[76..80].try_into().unwrap()) as usize * 512;
    let sectors = (result.len() - 512) / 512;
    assert!(sectors < 128);

    let target = (0..sectors)
        .find_map(|index| {
            let offset = fat + index * 4;
            let next = u32::from_le_bytes(result[offset..offset + 4].try_into().unwrap());
            (next < sectors as u32).then_some(next)
        })
        .expect("live FAT pointee");
    let offset = fat + sectors * 4;
    result[offset..offset + 4].copy_from_slice(&target.to_le_bytes());
    assert!(rhwp::parser::cfb_reader::CfbReader::open(&result).is_err());
    assert!(rhwp::parser::cfb_reader::LenientCfbReader::open(&result).is_ok());

    result
}

#[test]
fn issue_1391_memo_hwp5_truncated_framing_fails_strict_and_lenient_document_open() {
    let document = native_document(vec![Section {
        paragraphs: vec![
            annotated("Memo owner", 7, &["Memo payload"]),
            paragraph("Do not lose body"),
        ],
        ..Default::default()
    }]);
    let valid = rhwp::serializer::serialize_hwp(&document).unwrap();
    let records =
        rhwp::parser::record::Record::read_all(&serialize_section(&document.sections[0])).unwrap();
    let memo = records
        .iter()
        .position(|record| record.tag_id == rhwp::parser::tags::HWPTAG_MEMO_LIST)
        .unwrap();
    let prefix = rhwp::serializer::record_writer::write_records(&records[..memo]);
    let memo_header = u32::from(rhwp::parser::tags::HWPTAG_MEMO_LIST) | (1 << 10);
    let variants = [
        [
            (memo_header | (4 << 20)).to_le_bytes().as_slice(),
            &7_u16.to_le_bytes(),
        ]
        .concat(),
        [
            (memo_header | (0xfff << 20)).to_le_bytes().as_slice(),
            &4_u16.to_le_bytes(),
        ]
        .concat(),
        memo_header.to_le_bytes()[..2].to_vec(),
        [
            (memo_header | (4 << 20)).to_le_bytes().as_slice(),
            &7_u32.to_le_bytes(),
        ]
        .concat(),
    ];

    for (index, tail) in variants.iter().enumerate() {
        let section = [prefix.as_slice(), tail.as_slice()].concat();
        assert!(matches!(
            parse_body_text_section(&section),
            Err(rhwp::parser::body_text::BodyTextError::MemoStructure(_))
        ));

        let malformed = replace_section_stream(&valid, &section);
        assert!(
            rhwp::parse_document(&malformed).is_err(),
            "strict memo framing variant {index}"
        );

        let lenient = lenient_fixture(&malformed);
        assert!(
            rhwp::parse_document(&lenient).is_err(),
            "lenient memo framing variant {index}"
        );
    }
}

#[test]
fn issue_1391_memo_hwp5_duplicate_default_indexes_across_sections_keep_owners_and_updates() {
    let document = native_document(vec![
        Section {
            paragraphs: vec![annotated("First owner", 0, &["First memo"])],
            ..Default::default()
        },
        Section {
            paragraphs: vec![annotated("Second owner", 0, &["Second memo"])],
            ..Default::default()
        },
    ]);
    let bytes = rhwp::serializer::serialize_hwp(&document).unwrap();
    let mut parsed = rhwp::parse_document(&bytes).unwrap();
    assert_memo(&parsed.sections[0].paragraphs[0], 0, &["First memo"]);
    assert_memo(&parsed.sections[1].paragraphs[0], 0, &["Second memo"]);

    let unchanged =
        rhwp::parse_document(&rhwp::serializer::serialize_hwp(&parsed).unwrap()).unwrap();
    for (before, after) in parsed.sections.iter().zip(&unchanged.sections) {
        assert_eq!(
            before.raw_stream, after.raw_stream,
            "unchanged native section passthrough"
        );
    }

    let Control::Field(memo) = &mut parsed.sections[0].paragraphs[0].controls[0] else {
        panic!("first memo");
    };
    memo.memo_paragraphs[0].text = "Edited first memo".into();

    let saved = rhwp::serializer::serialize_hwp(&parsed).unwrap();
    let edited = rhwp::parse_document(&saved).unwrap();
    assert_memo(&edited.sections[0].paragraphs[0], 0, &["Edited first memo"]);
    assert_memo(&edited.sections[1].paragraphs[0], 0, &["Second memo"]);

    parsed.sections[0].paragraphs[0].controls.clear();
    parsed.sections[0].paragraphs[0].field_ranges.clear();

    let deleted = rhwp::parse_document(&rhwp::serializer::serialize_hwp(&parsed).unwrap()).unwrap();
    assert!(deleted.sections[0].paragraphs[0].controls.is_empty());
    assert_memo(&deleted.sections[1].paragraphs[0], 0, &["Second memo"]);
}

#[test]
fn issue_1391_memo_hwp5_plain_blank_body_and_normal_fields_are_untouched() {
    let mut normal = paragraph("Ordinary field");
    normal.controls.push(Control::Field(Field {
        field_type: FieldType::CrossRef,
        ctrl_id: rhwp::parser::tags::FIELD_CROSSREF,
        field_id: 88,
        command: "SOMETHING/ELSE".into(),
        ..Default::default()
    }));

    let mut blank = paragraph("");
    blank.has_para_text = false;
    blank.style_id = 9;
    let section = Section {
        paragraphs: vec![normal, blank],
        ..Default::default()
    };
    let parsed = roundtrip(&section);
    assert_eq!(parsed.paragraphs.len(), 2);
    assert_eq!(field(&parsed.paragraphs[0]).field_type, FieldType::CrossRef);
    assert_eq!(field(&parsed.paragraphs[0]).field_id, 88);
    assert_eq!(field(&parsed.paragraphs[0]).command, "SOMETHING/ELSE");
    assert!(parsed.paragraphs[1].text.is_empty());
    assert_eq!(parsed.paragraphs[1].style_id, 9);
}

fn memo_mut(para: &mut Paragraph) -> &mut Field {
    para.controls
        .iter_mut()
        .find_map(|control| match control {
            Control::Field(field) if field.field_type == FieldType::Memo => Some(field),
            _ => None,
        })
        .expect("memo owner")
}

#[test]
fn issue_1391_memo_hwp5_sealed_public_section_saves_memo_update_and_deletion() {
    let original = rhwp::serializer::serialize_hwp(&native_document(vec![Section {
        paragraphs: vec![annotated("Section owner", 81, &["Loaded memo"])],
        ..Default::default()
    }]))
    .unwrap();
    for delete in [false, true] {
        // Direct native parsing seals the tail even without DocumentCore fixups.
        let mut loaded = rhwp::parser::parse_hwp(&original).unwrap();
        let section = &mut loaded.sections[0];
        if delete {
            section.paragraphs[0].controls.clear();
            section.paragraphs[0].field_ranges.clear();
        } else {
            memo_mut(&mut section.paragraphs[0]).memo_paragraphs[0].text = "Edited memo".into();
        }
        let stream = serialize_section(section);
        let reopened = parse_body_text_section(&stream).unwrap();
        if delete {
            assert!(reopened.paragraphs[0].controls.is_empty());
            let records = rhwp::parser::record::Record::read_all(&stream).unwrap();
            assert!(records
                .iter()
                .all(|r| r.tag_id != rhwp::parser::tags::HWPTAG_MEMO_LIST));
        } else {
            assert_memo(&reopened.paragraphs[0], 81, &["Edited memo"]);
        }
    }
}

fn caption_owner(shape: &rhwp::model::shape::ShapeObject) -> &Paragraph {
    use rhwp::model::shape::ShapeObject;
    let caption = match shape {
        ShapeObject::Chart(chart) => {
            assert!(
                chart.drawing.caption.is_none(),
                "native parser moves chart caption"
            );
            &chart.caption
        }
        ShapeObject::Ole(ole) => {
            assert!(
                ole.drawing.caption.is_none(),
                "native parser moves OLE caption"
            );
            &ole.caption
        }
        _ => panic!("chart or OLE"),
    };
    &caption.as_ref().expect("native caption").paragraphs[0]
}

fn caption_owner_mut(shape: &mut rhwp::model::shape::ShapeObject) -> &mut Paragraph {
    use rhwp::model::shape::ShapeObject;
    let caption = match shape {
        ShapeObject::Chart(chart) => &mut chart.caption,
        ShapeObject::Ole(ole) => &mut ole.caption,
        _ => panic!("chart or OLE"),
    };
    &mut caption.as_mut().expect("native caption").paragraphs[0]
}

#[test]
fn issue_1391_memo_hwp5_native_chart_and_ole_caption_memo_update_and_deletion() {
    use rhwp::model::shape::{Caption, ChartShape, OleShape, ShapeObject};
    for chart in [false, true] {
        let caption = Some(Caption {
            paragraphs: vec![annotated("Caption owner", 82, &["Caption memo"])],
            ..Default::default()
        });
        let shape = if chart {
            let mut shape = ChartShape {
                caption,
                ..Default::default()
            };
            shape.drawing.shape_attr.ctrl_id = rhwp::parser::tags::SHAPE_RECT_ID;
            ShapeObject::Chart(Box::new(shape))
        } else {
            ShapeObject::Ole(Box::new(OleShape {
                caption,
                ..Default::default()
            }))
        };
        let mut body = paragraph("Shape owner");
        body.controls.push(Control::Shape(Box::new(shape)));
        let original = rhwp::serializer::serialize_hwp(&native_document(vec![Section {
            paragraphs: vec![body],
            ..Default::default()
        }]))
        .unwrap();
        for delete in [false, true] {
            let mut loaded = rhwp::parse_document(&original).unwrap();
            let Control::Shape(shape) = &mut loaded.sections[0].paragraphs[0].controls[0] else {
                panic!("native shape");
            };
            assert_memo(caption_owner(shape), 82, &["Caption memo"]);
            let owner = caption_owner_mut(shape);
            if delete {
                owner.controls.clear();
                owner.field_ranges.clear();
            } else {
                memo_mut(owner).memo_paragraphs[0].text = "Changed caption memo".into();
            }
            let saved = rhwp::serializer::serialize_hwp(&loaded).unwrap();
            let reopened =
                rhwp::parse_document(&saved).expect("edited native caption remains parseable");
            let Control::Shape(shape) = &reopened.sections[0].paragraphs[0].controls[0] else {
                panic!("reopened shape");
            };
            let owner = caption_owner(shape);
            if delete {
                assert!(owner.controls.is_empty());
                let records = rhwp::parser::record::Record::read_all(
                    reopened.sections[0].raw_stream.as_deref().unwrap(),
                )
                .unwrap();
                assert!(records
                    .iter()
                    .all(|r| r.tag_id != rhwp::parser::tags::HWPTAG_MEMO_LIST));
            } else {
                assert_memo(owner, 82, &["Changed caption memo"]);
            }
        }
    }
}

#[test]
fn issue_1391_memo_hwp5_memo_rewrite_preserves_native_header_and_line_metadata() {
    let rows = vec![LineSeg {
        text_start: 0,
        vertical_pos: 1440,
        line_height: 900,
        text_height: 900,
        baseline_distance: 765,
        line_spacing: 540,
        column_start: 0,
        segment_width: 48188,
        tag: 0x60000,
    }];
    for nested in [false, true] {
        let (original, expected_header) =
            native_memo_metadata_fixture(&[0x78, 0x56, 0x34, 0x12, 0x9a, 0xbc], &rows, nested);
        let mut loaded = rhwp::parse_document(&original).unwrap();
        let owner = if nested {
            let Control::Table(table) = &mut loaded.sections[0].paragraphs[0].controls[0] else {
                panic!("nested owner");
            };
            &mut table.cells[0].paragraphs[0]
        } else {
            &mut loaded.sections[0].paragraphs[0]
        };
        memo_mut(owner).memo_paragraphs[0].text = "Rewritten native memo".into();
        // A changed memo projection forces MEMO_LIST through the writer.
        let rebuilt = forced_rebuild(&loaded);
        let owner = if nested {
            let Control::Table(table) = &rebuilt.sections[0].paragraphs[0].controls[0] else {
                panic!("rebuilt nested owner");
            };
            &table.cells[0].paragraphs[0]
        } else {
            &rebuilt.sections[0].paragraphs[0]
        };
        let memo = &field(owner).memo_paragraphs[0];
        assert_eq!(memo.text, "Rewritten native memo");
        assert_eq!(memo.raw_header_extra, expected_header);
        assert_eq!(
            serde_json::to_value(&memo.line_segs).unwrap(),
            serde_json::to_value(&rows).unwrap()
        );
    }
}

fn replace_native_section(original: &[u8], section_index: usize, section: &[u8]) -> Vec<u8> {
    use std::io::{Cursor, Read, Write};
    let mut container = cfb::CompoundFile::open(Cursor::new(original.to_vec())).unwrap();
    let mut header = Vec::new();
    container
        .open_stream("/FileHeader")
        .unwrap()
        .read_to_end(&mut header)
        .unwrap();
    let payload = if header[36] & 1 != 0 {
        let mut encoder =
            flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(section).unwrap();
        encoder.finish().unwrap()
    } else {
        section.to_vec()
    };
    container
        .create_stream(format!("/BodyText/Section{section_index}"))
        .unwrap()
        .write_all(&payload)
        .unwrap();
    container.into_inner().into_inner()
}

#[test]
fn issue_1391_memo_hwp5_public_section_edit_keeps_foreign_memo_and_independent_tail() {
    use rhwp::model::document::RawRecord;
    use rhwp::parser::{record::Record, tags};
    let document = native_document(vec![
        Section {
            paragraphs: vec![annotated("Foreign owner", 91, &["Foreign memo"])],
            ..Default::default()
        },
        Section {
            paragraphs: vec![annotated("Local owner", 92, &["Local memo"])],
            ..Default::default()
        },
    ]);
    let original = rhwp::serializer::serialize_hwp(&document).unwrap();
    let mut loaded = rhwp::parse_document(&original).unwrap();
    // A native extension master and its opaque child are not memo-owned.
    let mut header = vec![0; 20];
    header[..2].copy_from_slice(&1u16.to_le_bytes());
    header[8..12].copy_from_slice(&1000u32.to_le_bytes());
    header[12..16].copy_from_slice(&2000u32.to_le_bytes());
    header[18..20].copy_from_slice(&4u16.to_le_bytes());
    let mut independent = vec![RawRecord {
        tag_id: tags::HWPTAG_LIST_HEADER,
        level: 1,
        data: header,
    }];
    let master_stream = serialize_section(&Section {
        paragraphs: vec![paragraph("Independent master")],
        ..Default::default()
    });
    independent.extend(
        Record::read_all(&master_stream)
            .unwrap()
            .into_iter()
            .map(|r| RawRecord {
                tag_id: r.tag_id,
                level: r.level + 1,
                data: r.data,
            }),
    );
    independent.push(RawRecord {
        tag_id: 511,
        level: 2,
        data: vec![0x13, 0x57, 0x9b],
    });
    loaded.sections[1]
        .trailing_lists
        .as_mut()
        .unwrap()
        .records
        .extend(independent.clone());
    // Load a native stream with the extra list so ownership/projection are sealed normally.
    loaded.sections[1].raw_stream = None;
    let fixture = rhwp::serializer::serialize_hwp(&loaded).unwrap();
    for delete in [false, true] {
        let mut loaded = rhwp::parse_document(&fixture).unwrap();
        assert_eq!(loaded.sections[1].section_def.master_pages.len(), 1);
        assert_eq!(
            loaded.sections[1].section_def.master_pages[0].paragraphs[0].text,
            "Independent master"
        );
        let section = &mut loaded.sections[1];
        if delete {
            section.paragraphs[0].controls.clear();
            section.paragraphs[0].field_ranges.clear();
        } else {
            memo_mut(&mut section.paragraphs[0]).memo_paragraphs[0].text =
                "Changed local memo".into();
        }
        let stream = serialize_section(section);
        let records = Record::read_all(&stream).unwrap();
        let independent_start = records
            .iter()
            .position(|r| r.tag_id == tags::HWPTAG_LIST_HEADER && r.data == independent[0].data)
            .unwrap();
        for (actual, expected) in records[independent_start..independent_start + independent.len()]
            .iter()
            .zip(&independent)
        {
            assert_eq!(
                (actual.tag_id, actual.level, &actual.data),
                (expected.tag_id, expected.level, &expected.data)
            );
        }
        let saved = replace_native_section(&fixture, 1, &stream);
        let reopened =
            rhwp::parse_document(&saved).expect("foreign memo retains its earlier-section owner");
        assert_memo(&reopened.sections[0].paragraphs[0], 91, &["Foreign memo"]);
        assert_eq!(
            reopened.sections[1].section_def.master_pages[0].paragraphs[0].text,
            "Independent master"
        );
        if delete {
            assert!(reopened.sections[1].paragraphs[0].controls.is_empty());
        } else {
            assert_memo(
                &reopened.sections[1].paragraphs[0],
                92,
                &["Changed local memo"],
            );
        }
        let count = records
            .iter()
            .filter(|r| r.tag_id == tags::HWPTAG_MEMO_LIST)
            .count();
        assert_eq!(count, if delete { 1 } else { 2 });
    }
}

fn move_native_memo_owner(source: usize) {
    use rhwp::parser::{record::Record, tags};
    let target = 1 - source;
    let mut sections = vec![
        Section {
            paragraphs: vec![paragraph("앞 구역 본문")],
            ..Default::default()
        },
        Section {
            paragraphs: vec![paragraph("뒤 구역 본문")],
            ..Default::default()
        },
    ];
    sections[source].paragraphs = vec![annotated("이동할 대상", 93, &["이동한 메모", "둘째 줄"])];
    let original = rhwp::serializer::serialize_hwp(&native_document(sections)).unwrap();
    let mut loaded = rhwp::parse_document(&original).unwrap();
    assert_memo(
        &loaded.sections[source].paragraphs[0],
        93,
        &["이동한 메모", "둘째 줄"],
    );
    let before = loaded.sections[source].paragraphs[0].clone();
    // Move the owner without changing the field or its document-wide order.
    let (first, last) = loaded.sections.split_at_mut(1);
    std::mem::swap(&mut first[0].paragraphs, &mut last[0].paragraphs);
    let saved = rhwp::serializer::serialize_hwp(&loaded).unwrap();
    let reopened = rhwp::parse_document(&saved).expect("moved native memo owner reopens");
    assert_eq!(reopened.sections[target].paragraphs[0].text, before.text);
    assert_memo(
        &reopened.sections[target].paragraphs[0],
        93,
        &["이동한 메모", "둘째 줄"],
    );
    assert!(reopened.sections[source].paragraphs[0]
        .controls
        .iter()
        .all(|c| !matches!(c, Control::Field(f) if f.field_type == FieldType::Memo)));
    let count: usize = reopened
        .sections
        .iter()
        .map(|section| {
            Record::read_all(section.raw_stream.as_deref().unwrap())
                .unwrap()
                .iter()
                .filter(|record| record.tag_id == tags::HWPTAG_MEMO_LIST)
                .count()
        })
        .sum();
    assert_eq!(
        count, 1,
        "one native list per memo owner after a section move"
    );
}

#[test]
fn issue_1391_memo_hwp5_move_owner_from_final_section_keeps_memo() {
    move_native_memo_owner(1);
}

#[test]
fn issue_1391_memo_hwp5_move_owner_to_final_section_keeps_memo() {
    move_native_memo_owner(0);
}

#[test]
fn issue_1391_memo_hwp5_public_section_duplicate_indexes_keep_foreign_owners() {
    use rhwp::parser::{record::Record, tags};
    for index in [0, 94] {
        let document = native_document(vec![
            Section {
                paragraphs: vec![annotated("앞 대상", index, &["앞 메모"])],
                ..Default::default()
            },
            Section {
                paragraphs: vec![annotated("뒤 대상", index, &["뒤 메모"])],
                ..Default::default()
            },
        ]);
        let original = rhwp::serializer::serialize_hwp(&document).unwrap();
        let loaded = rhwp::parse_document(&original).unwrap();
        let first = Record::read_all(loaded.sections[0].raw_stream.as_deref().unwrap()).unwrap();
        let last = Record::read_all(loaded.sections[1].raw_stream.as_deref().unwrap()).unwrap();
        let tail_start = last
            .iter()
            .position(|record| record.tag_id == tags::HWPTAG_MEMO_LIST)
            .unwrap();
        // Both native tail locations are valid; identical indexes attach in field order.
        for tail_section in [0, 1] {
            let fixture = if tail_section == 0 {
                let mut earlier = first.clone();
                earlier.extend_from_slice(&last[tail_start..]);
                let fixture = replace_native_section(
                    &original,
                    0,
                    &rhwp::serializer::record_writer::write_records(&earlier),
                );
                replace_native_section(
                    &fixture,
                    1,
                    &rhwp::serializer::record_writer::write_records(&last[..tail_start]),
                )
            } else {
                original.clone()
            };
            for delete in [false, true] {
                let mut loaded = rhwp::parse_document(&fixture).expect("mixed native tail loads");
                assert_memo(&loaded.sections[0].paragraphs[0], index, &["앞 메모"]);
                assert_memo(&loaded.sections[1].paragraphs[0], index, &["뒤 메모"]);
                let section = &mut loaded.sections[tail_section];
                if delete {
                    section.paragraphs[0].controls.clear();
                    section.paragraphs[0].field_ranges.clear();
                } else {
                    memo_mut(&mut section.paragraphs[0]).memo_paragraphs[0].text =
                        "바꾼 메모".into();
                }
                let stream = serialize_section(section);
                let count = Record::read_all(&stream)
                    .unwrap()
                    .iter()
                    .filter(|record| record.tag_id == tags::HWPTAG_MEMO_LIST)
                    .count();
                assert_eq!(count, if delete { 1 } else { 2 });
                let saved = replace_native_section(&fixture, tail_section, &stream);
                let reopened =
                    rhwp::parse_document(&saved).expect("mixed duplicate-index tail reopens");
                let foreign = 1 - tail_section;
                assert_memo(
                    &reopened.sections[foreign].paragraphs[0],
                    index,
                    &[if foreign == 0 {
                        "앞 메모"
                    } else {
                        "뒤 메모"
                    }],
                );
                if delete {
                    assert!(reopened.sections[tail_section].paragraphs[0]
                        .controls
                        .is_empty());
                } else {
                    assert_memo(
                        &reopened.sections[tail_section].paragraphs[0],
                        index,
                        &["바꾼 메모"],
                    );
                }
            }
        }
    }
}

fn tracked_memo_document() -> Document {
    use rhwp::model::document::RawRecord;
    use rhwp::model::paragraph::RangeTag;

    let mut changed = paragraph("삭제유지");
    changed.range_tags.push(RangeTag {
        start: 0,
        end: 2,
        tag: 0x1100_0001,
    });
    let mut document = native_document(vec![Section {
        paragraphs: vec![
            annotated("첫 대상", 101, &["첫 메모", ""]),
            annotated("둘째 대상", 102, &["둘째 메모"]),
            changed,
        ],
        ..Default::default()
    }]);
    let mut change = vec![0; 26];
    change[..4].copy_from_slice(&0x11_u32.to_le_bytes());
    change[22..26].copy_from_slice(&1_u32.to_le_bytes());
    document.doc_info.extra_records.push(RawRecord {
        tag_id: rhwp::parser::tags::HWPTAG_TRACK_CHANGE,
        level: 0,
        data: change,
    });
    document
}

#[test]
fn issue_1391_memo_hwp5_tracked_body_keeps_sibling_memo_lists() {
    let source = rhwp::serializer::serialize_hwp(&tracked_memo_document())
        .expect("변경 추적과 메모를 포함한 문서 저장");
    let parsed = rhwp::parse_document(&source).expect("변경 추적 본문과 메모 파싱");
    let section = &parsed.sections[0];
    assert_eq!(
        section.paragraphs.len(),
        3,
        "메모는 본문 문단에 포함하지 않음"
    );
    assert_eq!(
        section.paragraphs[2].text, "유지",
        "본문의 숨긴 변경 범위 적용"
    );
    assert_memo(&section.paragraphs[0], 101, &["첫 메모", ""]);
    assert_memo(&section.paragraphs[1], 102, &["둘째 메모"]);

    let saved = rhwp::serializer::serialize_hwp(&parsed).expect("변경 추적 메모 문서 왕복 저장");
    let reopened = rhwp::parse_document(&saved).expect("변경 추적 메모 문서 다시 열기");
    assert_eq!(
        reopened.sections[0].paragraphs[2].text, "유지",
        "왕복 후 가시 본문 유지"
    );
    assert_memo(&reopened.sections[0].paragraphs[0], 101, &["첫 메모", ""]);
    assert_memo(&reopened.sections[0].paragraphs[1], 102, &["둘째 메모"]);
    assert_eq!(
        section.raw_stream, reopened.sections[0].raw_stream,
        "원본 변경 이력과 메모 저장 범위 보존"
    );
}

#[test]
fn issue_1391_memo_hwp5_tracked_body_rejects_malformed_sibling_counts() {
    let document = tracked_memo_document();
    let valid = rhwp::serializer::serialize_hwp(&document).expect("정상 대조 문서 저장");
    let original =
        rhwp::parser::record::Record::read_all(&serialize_section(&document.sections[0]))
            .expect("정상 대조 문단 레코드 읽기");
    for count in [0_u32, 3] {
        let mut records = original.clone();
        let header = records
            .iter_mut()
            .find(|record| {
                record.tag_id == rhwp::parser::tags::HWPTAG_LIST_HEADER && record.level == 1
            })
            .expect("첫 메모 목록 헤더");
        header.data[..4].copy_from_slice(&count.to_le_bytes());
        let stream = rhwp::serializer::record_writer::write_records(&records);
        let malformed = replace_section_stream(&valid, &stream);
        assert!(
            matches!(
                rhwp::parse_document(&malformed),
                Err(rhwp::parser::ParseError::BodyTextError(
                    rhwp::parser::body_text::BodyTextError::MemoStructure(_)
                ))
            ),
            "변경 투영이 잘못된 메모 문단 수 {count}를 정상화하지 않음"
        );
    }
    let mut truncated = rhwp::serializer::record_writer::write_records(&original);
    truncated.pop();
    let malformed = replace_section_stream(&valid, &truncated);
    assert!(
        matches!(
            rhwp::parse_document(&malformed),
            Err(rhwp::parser::ParseError::BodyTextError(
                rhwp::parser::body_text::BodyTextError::MemoStructure(_)
            ))
        ),
        "변경 투영 전에 잘린 메모 레코드 거부"
    );
}

#[test]
fn issue_5169_memo_hwp5_viewtext_keeps_annotations_and_review_history() {
    fn memos(paragraphs: &[Paragraph], result: &mut Vec<(u32, String, u32, Vec<String>)>) {
        for para in paragraphs {
            for control in &para.controls {
                match control {
                    Control::Field(field) if field.field_type == FieldType::Memo => {
                        result.push((
                            field.memo_index,
                            field.command.clone(),
                            field.field_id,
                            field
                                .memo_paragraphs
                                .iter()
                                .map(|p| p.text.clone())
                                .collect(),
                        ));
                    }
                    Control::Table(table) => {
                        for cell in &table.cells {
                            memos(&cell.paragraphs, result);
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    fn annotations(document: &Document) -> Vec<(u32, String, u32, Vec<String>)> {
        let mut result = Vec::new();
        for section in &document.sections {
            memos(&section.paragraphs, &mut result);
        }
        result.sort_by_key(|memo| memo.0);
        result
    }

    let source = std::fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("samples/issue5169_viewtext_changetracking.hwp"),
    )
    .expect("변경 추적 원본 표본 읽기");
    let parsed = rhwp::parse_document(&source).expect("ViewText 변경 추적과 메모 파싱");
    assert!(
        annotations(&parsed).is_empty(),
        "삭제된 표의 메모 필드는 가시 주석으로 노출하지 않음"
    );
    let raw_document = native_document(
        parsed
            .sections
            .iter()
            .map(|section| {
                parse_body_text_section(section.raw_stream.as_deref().expect("원본 검토 이력"))
                    .expect("투영 전 메모와 소유 필드 파싱")
            })
            .collect(),
    );
    let source_annotations = annotations(&raw_document);
    assert_eq!(
        source_annotations
            .iter()
            .map(|memo| (
                memo.0,
                memo.3.iter().map(String::as_str).collect::<Vec<_>>()
            ))
            .collect::<Vec<_>>(),
        vec![
            (
                1,
                vec!["첫째날 세미나와 주제가 중복되어, 차별화 혹은 삭제 필요"]
            ),
            (3, vec!["의견 반영하여 변경함."]),
        ],
        "숨긴 두 메모의 내용과 소유 번호는 원본 검토 이력에 보존"
    );
    let saved = rhwp::serializer::serialize_hwp(&parsed).expect("ViewText 메모 문서 저장");
    let reopened = rhwp::parse_document(&saved).expect("ViewText 메모 문서 다시 열기");
    assert_eq!(
        annotations(&reopened),
        Vec::new(),
        "왕복 후에도 숨긴 메모가 가시 주석으로 나타나지 않음"
    );
    for (before, after) in parsed.sections.iter().zip(&reopened.sections) {
        assert_eq!(
            before.raw_stream, after.raw_stream,
            "원본 ViewText 변경 이력 보존"
        );
    }
}

fn all_hidden_final_section_with_foreign_memo() -> (Vec<u8>, Vec<rhwp::parser::record::Record>) {
    use rhwp::model::document::RawRecord;
    use rhwp::model::paragraph::RangeTag;
    use rhwp::parser::{record::Record, tags};

    let mut hidden = paragraph("삭제");
    hidden.range_tags.push(RangeTag {
        start: 0,
        end: 3,
        tag: 0x1100_0001,
    });
    let mut document = native_document(vec![
        Section {
            paragraphs: vec![annotated("유지된 소유자", 91, &["메모 전용 내용"])],
            ..Default::default()
        },
        Section {
            paragraphs: vec![hidden],
            ..Default::default()
        },
    ]);
    let mut change = vec![0; 26];
    change[..4].copy_from_slice(&0x11_u32.to_le_bytes());
    change[22..26].copy_from_slice(&1_u32.to_le_bytes());
    document.doc_info.extra_records.push(RawRecord {
        tag_id: tags::HWPTAG_TRACK_CHANGE,
        level: 0,
        data: change,
    });

    // Replace the final section with an explicit native stream so inserted
    // section-definition slots cannot change the hidden range's source axis.
    let mut records = Record::read_all(&serialize_section(&document.sections[1])).unwrap();
    let text = records
        .iter()
        .find(|r| r.tag_id == tags::HWPTAG_PARA_TEXT)
        .unwrap();
    assert_eq!(text.data.len() / 2, 3, "숨긴 본문 두 글자와 종료 문자");
    let owner_records = Record::read_all(&serialize_section(&document.sections[0])).unwrap();
    let tail_start = owner_records
        .iter()
        .position(|r| r.tag_id == tags::HWPTAG_MEMO_LIST)
        .unwrap();
    records.extend_from_slice(&owner_records[tail_start..]);
    let stream = rhwp::serializer::record_writer::write_records(&records);
    let source = rhwp::serializer::serialize_hwp(&document).expect("두 구역 문서 저장");
    (replace_native_section(&source, 1, &stream), records)
}

#[test]
fn issue_1391_review_all_hidden_section_does_not_publish_foreign_memo_text() {
    let (source, records) = all_hidden_final_section_with_foreign_memo();
    let parsed = rhwp::parse_document(&source).expect("숨긴 마지막 구역과 외부 메모 파싱");
    assert_eq!(parsed.sections.len(), 2, "구역 수 유지");
    assert_memo(&parsed.sections[0].paragraphs[0], 91, &["메모 전용 내용"]);
    assert!(
        parsed.sections[1].paragraphs.is_empty(),
        "모두 숨긴 구역은 메모 내용을 본문으로 표시하지 않음: {:?}",
        parsed.sections[1]
            .paragraphs
            .iter()
            .map(|p| p.text.as_str())
            .collect::<Vec<_>>()
    );
    assert!(
        parsed.sections[1].trailing_lists.is_some(),
        "메모 꼬리 소유권 보존"
    );
    let stream = rhwp::serializer::record_writer::write_records(&records);
    assert_eq!(
        parsed.sections[1].raw_stream.as_deref(),
        Some(stream.as_slice()),
        "숨긴 본문과 메모의 원본 레코드 보존"
    );
    let saved = rhwp::serializer::serialize_hwp(&parsed).expect("모두 숨긴 구역 저장");
    let reopened = rhwp::parse_document(&saved).expect("모두 숨긴 구역 다시 열기");
    assert!(
        reopened.sections[1].paragraphs.is_empty(),
        "왕복 후에도 메모 본문 누출 없음"
    );
    assert!(
        reopened.sections[1].trailing_lists.is_some(),
        "왕복 후에도 꼬리 소유권 유지"
    );
    assert_memo(&reopened.sections[0].paragraphs[0], 91, &["메모 전용 내용"]);
    for (before, after) in parsed.sections.iter().zip(&reopened.sections) {
        assert_eq!(
            before.raw_stream, after.raw_stream,
            "왕복 후 전체 변경 이력 보존"
        );
    }
    let mut container =
        cfb::CompoundFile::open(std::io::Cursor::new(saved)).expect("저장한 HWP 열기");
    container
        .remove_stream("/ViewText/Section1")
        .expect("최종 구역의 검토 이력 제거");
    let body_only = rhwp::parse_document(&container.into_inner().into_inner())
        .expect("BodyText만 있는 빈 최종 구역 파싱");
    assert!(
        body_only.sections[1].paragraphs.is_empty(),
        "ViewText가 없어도 메모는 본문이 아님"
    );
    assert!(
        body_only.sections[1].trailing_lists.is_some(),
        "BodyText의 메모 꼬리 소유권 유지"
    );
    assert_memo(
        &body_only.sections[0].paragraphs[0],
        91,
        &["메모 전용 내용"],
    );
    let hwpx = rhwp::serializer::serialize_hwpx(&parsed).expect("모두 숨긴 구역 HWPX 변환");
    let converted = rhwp::parser::hwpx::parse_hwpx(&hwpx).expect("변환한 HWPX 다시 열기");
    assert_eq!(converted.sections.len(), 2, "HWPX 변환 후 구역 수 유지");
    assert!(
        converted.sections[1]
            .paragraphs
            .iter()
            .all(|para| para.text.is_empty()),
        "HWPX의 빈 구역에 숨긴 본문이나 메모 내용이 나타나지 않음"
    );
    assert_memo(
        &converted.sections[0].paragraphs[0],
        91,
        &["메모 전용 내용"],
    );
}

#[test]
fn issue_1391_memo_hwp5_all_hidden_body_keeps_ordinary_blank_paragraph() {
    let (source, mut records) = all_hidden_final_section_with_foreign_memo();
    let tail_start = records
        .iter()
        .position(|record| record.tag_id == rhwp::parser::tags::HWPTAG_MEMO_LIST)
        .expect("메모 꼬리 시작");
    let blank = paragraph("");
    let blank_records = rhwp::parser::record::Record::read_all(&serialize_section(&Section {
        paragraphs: vec![blank],
        ..Default::default()
    }))
    .expect("일반 빈 문단 레코드");
    records.splice(tail_start..tail_start, blank_records);
    let stream = rhwp::serializer::record_writer::write_records(&records);
    let source = replace_native_section(&source, 1, &stream);
    let parsed = rhwp::parse_document(&source).expect("숨긴 문단과 일반 빈 문단 파싱");
    let section = &parsed.sections[1];
    assert_eq!(section.paragraphs.len(), 1, "숨긴 문단만 제거");
    assert!(section.paragraphs[0].text.is_empty(), "일반 빈 문단 유지");
    assert!(section.trailing_lists.is_some(), "메모 꼬리 소유권 유지");
    assert_memo(&parsed.sections[0].paragraphs[0], 91, &["메모 전용 내용"]);
}

#[test]
fn issue_1391_memo_hwp5_all_hidden_body_rejects_invalid_projection_and_owner() {
    use rhwp::parser::{body_text::BodyTextError, record::Record, tags, ParseError};

    let (source, records) = all_hidden_final_section_with_foreign_memo();
    for variant in 0..4 {
        let mut malformed = records.clone();
        match variant {
            0 => {
                let range = malformed
                    .iter_mut()
                    .find(|record| record.tag_id == tags::HWPTAG_PARA_RANGE_TAG)
                    .expect("숨긴 본문 범위");
                range.data[4..8].copy_from_slice(&4_u32.to_le_bytes());
            }
            1 => {
                let memo = malformed
                    .iter_mut()
                    .find(|record| record.tag_id == tags::HWPTAG_MEMO_LIST)
                    .expect("메모 꼬리");
                memo.data.copy_from_slice(&92_u32.to_le_bytes());
            }
            2 => {
                let list = malformed
                    .iter_mut()
                    .find(|record| record.tag_id == tags::HWPTAG_LIST_HEADER)
                    .expect("메모 문단 수");
                list.data[..4].copy_from_slice(&0_u32.to_le_bytes());
            }
            _ => {}
        }
        let mut stream = rhwp::serializer::record_writer::write_records(&malformed);
        if variant == 3 {
            stream.pop();
            assert!(Record::read_all(&stream).is_err(), "잘린 꼬리 대조");
        }
        let malformed = replace_native_section(&source, 1, &stream);
        let lenient = lenient_fixture(&malformed);
        for data in [&malformed, &lenient] {
            let error = rhwp::parse_document(data).expect_err("잘못된 투영과 소유권 거부");
            if variant == 0 {
                assert!(
                    matches!(
                        error,
                        ParseError::BodyTextError(BodyTextError::ChangeTrackingStructure(_))
                    ),
                    "가시 본문이 없어도 잘못된 변경 범위 거부"
                );
            } else {
                assert!(
                    matches!(
                        error,
                        ParseError::BodyTextError(BodyTextError::MemoStructure(_))
                    ),
                    "가시 본문이 없어도 메모 구조와 소유권 오류 {variant} 거부"
                );
            }
        }
    }
}

#[test]
fn issue_1391_memo_hwp5_tail_only_section_keeps_nested_body_classification() {
    use rhwp::parser::{record::Record, tags};

    let (_, records) = all_hidden_final_section_with_foreign_memo();
    let tail_start = records
        .iter()
        .position(|r| r.tag_id == tags::HWPTAG_MEMO_LIST)
        .expect("독립 메모 꼬리");
    let mut tail = records[tail_start..].to_vec();
    let stream = rhwp::serializer::record_writer::write_records(&tail);
    let parsed = parse_body_text_section(&stream).expect("본문이 없는 메모 꼬리 파싱");
    assert!(parsed.paragraphs.is_empty(), "독립 메모 문단은 본문이 아님");
    assert!(parsed.trailing_lists.is_some(), "독립 메모 꼬리 소유권");
    tail[1].data[..4].copy_from_slice(&0_u32.to_le_bytes());
    let malformed = rhwp::serializer::record_writer::write_records(&tail);
    assert!(
        matches!(
            parse_body_text_section(&malformed),
            Err(rhwp::parser::body_text::BodyTextError::MemoStructure(_))
        ),
        "본문이 없어도 선언하지 않은 메모 문단 거부"
    );

    let mut ordinary = Record::read_all(&serialize_section(&Section {
        paragraphs: vec![paragraph("일반 중첩 문단")],
        ..Default::default()
    }))
    .expect("일반 중첩 문단 대조");
    for record in &mut ordinary {
        record.level += 1;
    }
    let stream = rhwp::serializer::record_writer::write_records(&ordinary);
    let parsed = parse_body_text_section(&stream).expect("기존 중첩 문단 파싱");
    assert_eq!(
        parsed.paragraphs.len(),
        1,
        "메모 표식 없는 문단을 꼬리로 분류하지 않음"
    );
    assert_eq!(
        parsed.paragraphs[0].text, "일반 중첩 문단",
        "일반 문단 내용 유지"
    );
    assert!(
        parsed.trailing_lists.is_none(),
        "임의의 중첩 문단에 꼬리 소유권을 만들지 않음"
    );
}

fn tracked_ancestor_memo_fixture(
    hidden_owners: usize,
    same_identity: bool,
) -> (Vec<u8>, Vec<Vec<rhwp::parser::record::Record>>) {
    use rhwp::model::document::RawRecord;
    use rhwp::model::paragraph::RangeTag;
    use rhwp::parser::{record::Record, tags};

    let mut paragraphs = Vec::new();
    for index in 0..2 {
        let mut owner = annotated(
            "메모 대상",
            207,
            &[if index == 0 {
                "첫 원본 주석"
            } else {
                "둘째 원본 주석"
            }],
        );
        if !same_identity {
            memo_mut(&mut owner).field_id += index as u32;
        }
        if index < hidden_owners {
            let mut ancestor = paragraph("");
            ancestor
                .controls
                .push(Control::Table(Box::new(table(vec![owner], true))));
            ancestor.range_tags.push(RangeTag {
                start: 0,
                end: 9,
                tag: 0x1100_0001,
            });
            paragraphs.push(ancestor);
        } else {
            paragraphs.push(owner);
        }
    }
    let mut changed = paragraph("삭제유지");
    changed.range_tags.push(RangeTag {
        start: 0,
        end: 2,
        tag: 0x1100_0001,
    });
    paragraphs.push(changed);
    let mut document = native_document(vec![
        Section {
            paragraphs,
            ..Default::default()
        },
        Section {
            paragraphs: vec![paragraph("별도 꼬리 구역")],
            ..Default::default()
        },
    ]);
    let mut change = vec![0; 26];
    change[..4].copy_from_slice(&0x11_u32.to_le_bytes());
    change[22..26].copy_from_slice(&1_u32.to_le_bytes());
    document.doc_info.extra_records.push(RawRecord {
        tag_id: tags::HWPTAG_TRACK_CHANGE,
        level: 0,
        data: change,
    });
    let mut first =
        Record::read_all(&serialize_section(&document.sections[0])).expect("원본 소유 필드 레코드");
    let tail = first
        .iter()
        .position(|r| r.tag_id == tags::HWPTAG_MEMO_LIST)
        .expect("원본 메모 꼬리");
    let memos = first.split_off(tail);
    let mut final_section =
        Record::read_all(&serialize_section(&document.sections[1])).expect("독립 꼬리 구역 레코드");
    final_section.extend(memos);
    let records = vec![first, final_section];
    let mut source = rhwp::serializer::serialize_hwp(&document).expect("원본 검토 문서 저장");
    for (index, section) in records.iter().enumerate() {
        source = replace_native_section(
            &source,
            index,
            &rhwp::serializer::record_writer::write_records(section),
        );
    }
    (source, records)
}

#[test]
fn issue_1391_memo_hwp5_hidden_ancestor_keeps_visible_duplicate_index_owner() {
    let (source, _) = tracked_ancestor_memo_fixture(1, false);
    let parsed =
        rhwp::parse_document(&source).expect("숨긴 조상과 같은 메모 번호의 가시 필드 파싱");
    let section = &parsed.sections[0];
    assert_eq!(section.paragraphs.len(), 2, "삭제된 표만 제거");
    assert_eq!(section.paragraphs[1].text, "유지", "본문 가시 범위 적용");
    let memo = field(&section.paragraphs[0]);
    assert_eq!(memo.field_id, 308, "가시 필드의 원본 식별자 유지");
    assert_eq!(
        memo.memo_paragraphs[0].text, "둘째 원본 주석",
        "숨긴 첫 소유자의 주석을 가시 필드에 붙이지 않음"
    );
    let saved = rhwp::serializer::serialize_hwp(&parsed).expect("숨긴 조상과 메모 저장");
    let reopened = rhwp::parse_document(&saved).expect("검토 이력을 유지한 문서 다시 열기");
    assert_eq!(
        field(&reopened.sections[0].paragraphs[0]).memo_paragraphs[0].text,
        "둘째 원본 주석",
        "왕복 후 주석 소유 유지"
    );
    for (before, after) in parsed.sections.iter().zip(&reopened.sections) {
        assert_eq!(
            before.raw_stream, after.raw_stream,
            "숨긴 주석을 포함한 전체 원본 이력 보존"
        );
    }
    let mut container =
        cfb::CompoundFile::open(std::io::Cursor::new(saved)).expect("검토 이력 컨테이너 열기");
    for index in 0..parsed.sections.len() {
        container
            .remove_stream(format!("/ViewText/Section{index}"))
            .expect("검토 이력 대신 가시 본문 확인");
    }
    let body_only = rhwp::parse_document(&container.into_inner().into_inner())
        .expect("가시 BodyText만 다시 열기");
    assert_eq!(
        field(&body_only.sections[0].paragraphs[0]).memo_paragraphs[0].text,
        "둘째 원본 주석",
        "BodyText에도 가시 필드의 주석만 유지"
    );
    let tail = body_only.sections[1]
        .trailing_lists
        .as_ref()
        .expect("가시 주석 꼬리");
    assert_eq!(
        tail.records
            .iter()
            .filter(|r| r.tag_id == rhwp::parser::tags::HWPTAG_MEMO_LIST)
            .count(),
        1,
        "숨긴 주석은 BodyText에 저장하지 않음"
    );
    let hwpx = rhwp::serializer::serialize_hwpx(&parsed).expect("가시 메모 HWPX 변환");
    let converted = rhwp::parser::hwpx::parse_hwpx(&hwpx).expect("가시 HWPX 다시 열기");
    assert_eq!(
        field(&converted.sections[0].paragraphs[0]).memo_paragraphs[0].text,
        "둘째 원본 주석",
        "HWPX의 가시 메모 소유 유지"
    );
}

#[test]
fn issue_1391_memo_hwp5_tracked_exact_duplicate_owners_are_unambiguous_when_uniformly_visible() {
    for hidden in [0, 2] {
        let (source, _) = tracked_ancestor_memo_fixture(hidden, true);
        let parsed = rhwp::parse_document(&source).expect("모두 가시 또는 숨긴 중복 소유자 파싱");
        if hidden == 0 {
            assert_memo(&parsed.sections[0].paragraphs[0], 207, &["첫 원본 주석"]);
            assert_memo(&parsed.sections[0].paragraphs[1], 207, &["둘째 원본 주석"]);
        } else {
            assert_eq!(parsed.sections[0].paragraphs.len(), 1, "숨긴 두 표를 제거");
            assert_eq!(
                parsed.sections[0].paragraphs[0].text, "유지",
                "가시 본문만 유지"
            );
            assert!(
                parsed.sections[0].paragraphs[0].controls.is_empty(),
                "숨긴 주석 필드가 나타나지 않음"
            );
        }
        let saved = rhwp::serializer::serialize_hwp(&parsed)
            .expect("일관된 가시성을 가진 중복 소유자 저장");
        let reopened = rhwp::parse_document(&saved).expect("중복 소유자 다시 열기");
        for (before, after) in parsed.sections.iter().zip(&reopened.sections) {
            assert_eq!(
                before.raw_stream, after.raw_stream,
                "중복 소유자와 전체 주석 이력 보존"
            );
        }
    }
}

#[test]
fn issue_1391_memo_hwp5_hidden_ancestor_rejects_ambiguous_and_unowned_raw_lists() {
    use rhwp::parser::{body_text::BodyTextError, record::Record, tags, ParseError};
    let (ambiguous, _) = tracked_ancestor_memo_fixture(1, true);
    let (source, records) = tracked_ancestor_memo_fixture(1, false);
    let mut invalid = vec![ambiguous];
    let mut wrong_index = records[1].clone();
    wrong_index
        .iter_mut()
        .find(|r| r.tag_id == tags::HWPTAG_MEMO_LIST)
        .expect("숨긴 소유자의 주석 꼬리")
        .data
        .copy_from_slice(&208_u32.to_le_bytes());
    invalid.push(replace_native_section(
        &source,
        1,
        &rhwp::serializer::record_writer::write_records(&wrong_index),
    ));
    let mut missing_owner = records[0].clone();
    let field = missing_owner
        .iter()
        .position(|r| {
            if r.tag_id != tags::HWPTAG_CTRL_HEADER || r.level <= 1 || r.data.len() < 4 {
                return false;
            }
            let ctrl_id = u32::from_le_bytes(r.data[..4].try_into().unwrap());
            matches!(
                rhwp::parser::control::parse_control(ctrl_id, &r.data[4..], &[]),
                Control::Field(field)
                    if field.field_id == 307
                        && field.command == r"MEMO/65535/207/123/456/Synthetic Author/\;;"
            )
        })
        .expect("숨긴 조상의 원본 메모 필드");
    missing_owner.remove(field);
    let stream = rhwp::serializer::record_writer::write_records(&missing_owner);
    assert!(
        Record::read_all(&stream).is_ok(),
        "잘린 프레이밍이 아닌 원본 소유권 오류"
    );
    invalid.push(replace_native_section(&source, 0, &stream));
    for malformed in invalid {
        let lenient = lenient_fixture(&malformed);
        for data in [&malformed, &lenient] {
            assert!(
                matches!(
                    rhwp::parse_document(data),
                    Err(ParseError::BodyTextError(BodyTextError::MemoStructure(_)))
                ),
                "모호한 가시 소유권과 원본의 고아 주석은 엄격 및 관대한 경로에서 모두 거부"
            );
        }
    }
}

#[test]
fn issue_1391_memo_hwp5_hidden_ancestor_does_not_replay_provisional_local_note() {
    use rhwp::parser::tags;
    let (source, mut records) = tracked_ancestor_memo_fixture(1, false);
    let final_section = &mut records[1];
    let second = final_section
        .iter()
        .enumerate()
        .filter(|(_, r)| r.tag_id == tags::HWPTAG_MEMO_LIST)
        .nth(1)
        .map(|(i, _)| i)
        .expect("가시 필드의 두 번째 목록");
    final_section.truncate(second);
    let source = replace_native_section(
        &source,
        1,
        &rhwp::serializer::record_writer::write_records(final_section),
    );
    let parsed = rhwp::parse_document(&source).expect("숨긴 원본 소유자만 목록을 가진 문서 파싱");
    assert!(
        field(&parsed.sections[0].paragraphs[0])
            .memo_paragraphs
            .is_empty(),
        "목록 없는 가시 필드에 숨긴 소유자의 주석을 임시 부착 결과로 남기지 않음"
    );
    let saved = rhwp::serializer::serialize_hwp(&parsed).expect("숨긴 소유자의 주석 이력 저장");
    let reopened = rhwp::parse_document(&saved).expect("숨긴 주석 이력을 가진 문서 다시 열기");
    assert!(
        field(&reopened.sections[0].paragraphs[0])
            .memo_paragraphs
            .is_empty(),
        "왕복 후에도 숨긴 주석 내용 누출 없음"
    );
    for (before, after) in parsed.sections.iter().zip(&reopened.sections) {
        assert_eq!(
            before.raw_stream, after.raw_stream,
            "원본 주석 이력은 손실 없이 유지"
        );
    }
}

fn assert_hidden_ancestor_memo_after_body_edits(hidden_only_note: bool) {
    use rhwp::parser::tags;

    let (mut source, mut records) = tracked_ancestor_memo_fixture(1, false);
    if hidden_only_note {
        let second = records[1]
            .iter()
            .enumerate()
            .filter(|(_, record)| record.tag_id == tags::HWPTAG_MEMO_LIST)
            .nth(1)
            .map(|(index, _)| index)
            .expect("가시 소유자의 두 번째 메모 목록");
        records[1].truncate(second);
        source = replace_native_section(
            &source,
            1,
            &rhwp::serializer::record_writer::write_records(&records[1]),
        );
    }
    let original = rhwp::parse_document(&source).expect("숨긴 메모 소유권 대조 문서 파싱");
    let expected_notes = if hidden_only_note {
        vec![]
    } else {
        vec!["둘째 원본 주석"]
    };
    for edited in 0..4 {
        let mut document = original.clone();
        if edited & 1 != 0 {
            document.sections[0].paragraphs[1]
                .text
                .push_str(" 소유 구역 편집");
        }
        if edited & 2 != 0 {
            document.sections[1].paragraphs[0]
                .text
                .push_str(" 꼬리 구역 편집");
        }
        let saved = rhwp::serializer::serialize_hwp(&document).expect("본문 편집 문서 저장");
        let reopened = rhwp::parse_document(&saved).expect("본문 편집 문서를 유효하게 다시 열기");
        if edited == 0 {
            for (before, after) in original.sections.iter().zip(&reopened.sections) {
                assert_eq!(
                    before.raw_stream, after.raw_stream,
                    "무편집 대조군은 숨긴 메모를 포함한 전체 이력 보존"
                );
            }
        }
        let mut container =
            cfb::CompoundFile::open(std::io::Cursor::new(saved)).expect("저장 컨테이너 열기");
        for index in 0..document.sections.len() {
            let path = format!("/ViewText/Section{index}");
            if container.exists(&path) {
                container
                    .remove_stream(path)
                    .expect("검토 이력 대신 가시 본문 검사");
            }
        }
        let body_only = rhwp::parse_document(&container.into_inner().into_inner())
            .expect("본문 편집 후 가시 BodyText만 유효하게 다시 열기");
        for result in [&reopened, &body_only] {
            assert_eq!(result.sections.len(), 2, "본문 편집 후 구역 수 보존");
            assert_eq!(
                result.sections[0].paragraphs.len(),
                2,
                "숨긴 조상 본문을 재노출하지 않음"
            );
            assert_eq!(
                result.sections[0].paragraphs[1].text, document.sections[0].paragraphs[1].text,
                "소유 구역에서 실제 편집한 본문을 저장하고 다시 읽음"
            );
            assert_eq!(
                result.sections[1].paragraphs[0].text, document.sections[1].paragraphs[0].text,
                "꼬리 구역에서 실제 편집한 본문을 저장하고 다시 읽음"
            );
            let memo = field(&result.sections[0].paragraphs[0]);
            assert_eq!(memo.field_id, 308, "가시 메모 필드의 원본 식별자 유지");
            assert_eq!(
                memo.memo_paragraphs
                    .iter()
                    .map(|p| p.text.as_str())
                    .collect::<Vec<_>>(),
                expected_notes,
                "본문 편집 뒤 숨긴 소유자의 메모를 가시 필드에 붙이지 않음"
            );
        }
        let visible_lists = body_only
            .sections
            .iter()
            .filter_map(|section| section.trailing_lists.as_ref())
            .flat_map(|tail| &tail.records)
            .filter(|record| record.tag_id == tags::HWPTAG_MEMO_LIST)
            .count();
        assert_eq!(
            visible_lists,
            usize::from(!hidden_only_note || edited != 0),
            "BodyText에 가시 메모만 저장하며 편집 후에는 주석 없는 가시 필드의 빈 목록 유지"
        );
    }
}

/// 소유 구역, 꼬리 구역, 두 구역의 본문 편집 후에도 가시 주석과 편집 내용을 보존한다.
#[test]
fn issue_1391_memo_hwp5_body_edits_keep_visible_duplicate_index_note() {
    assert_hidden_ancestor_memo_after_body_edits(false);
}

/// 숨긴 소유자만 주석을 가진 경우 어느 구역을 편집해도 그 주석을 노출하지 않는다.
#[test]
fn issue_1391_memo_hwp5_body_edits_do_not_expose_hidden_only_note() {
    assert_hidden_ancestor_memo_after_body_edits(true);
}

/// 숨긴 메모 소유자가 없으면 본문 편집 뒤에도 독립 원본 꼬리의 보존 계약을 유지한다.
#[test]
fn issue_1391_memo_hwp5_body_edit_keeps_visible_only_native_tail() {
    let (source, _) = tracked_ancestor_memo_fixture(0, false);
    let mut document = rhwp::parse_document(&source).expect("모두 가시인 중복 메모 소유자 파싱");
    let original_tail = document.sections[1].raw_stream.clone();
    document.sections[0].paragraphs[2]
        .text
        .push_str(" 본문 편집");
    let saved =
        rhwp::serializer::serialize_hwp(&document).expect("모두 가시인 메모의 본문 편집 저장");
    let reopened = rhwp::parse_document(&saved).expect("가시 원본 꼬리를 가진 문서 다시 열기");
    assert_eq!(
        reopened.sections[0].paragraphs[2].text, "유지 본문 편집",
        "본문 편집 내용 보존"
    );
    assert_eq!(
        reopened.sections[1].raw_stream, original_tail,
        "숨긴 메모가 없으면 독립 원본 꼬리 유지"
    );
    for (index, expected) in ["첫 원본 주석", "둘째 원본 주석"].iter().enumerate() {
        let memo = field(&reopened.sections[0].paragraphs[index]);
        assert_eq!(
            memo.memo_paragraphs.len(),
            1,
            "각 가시 필드에 주석 하나 유지"
        );
        assert_eq!(
            memo.memo_paragraphs[0].text, *expected,
            "가시 중복 번호의 주석 소유 유지"
        );
    }
}
