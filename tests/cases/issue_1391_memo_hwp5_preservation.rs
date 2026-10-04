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
