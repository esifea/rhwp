//! Source line cache provenance across repeated text edits and HWP roundtrips.

use rhwp::document_core::DocumentCore;
use rhwp::model::document::{Document, Section};
use rhwp::model::paragraph::{CharShapeRef, Paragraph};

#[test]
fn missing_source_cache_stays_layout_only_across_repeated_text_edits() {
    const TEXT: &str = "Synthetic cache-free owner";
    let mut document = Document::default();
    document.doc_info.char_shapes = vec![Default::default()];
    document.doc_info.para_shapes = vec![Default::default()];
    document.doc_info.styles = vec![Default::default()];
    document.sections.push(Section {
        paragraphs: vec![Paragraph {
            text: TEXT.into(),
            char_count: TEXT.encode_utf16().count() as u32 + 1,
            char_offsets: (0..TEXT.encode_utf16().count() as u32).collect(),
            char_shapes: vec![CharShapeRef {
                start_pos: 0,
                char_shape_id: 0,
            }],
            ..Default::default()
        }],
        ..Default::default()
    });
    let source = rhwp::serializer::serialize_hwpx(&document).expect("author missing source cache");
    assert!(
        rhwp::parse_document(&source).unwrap().sections[0].paragraphs[0].line_segs.is_empty()
    );
    let mut core = DocumentCore::from_bytes(&source).expect("layout cache-free source");
    for prefix in ["First ", "Second "] {
        core.insert_text_native(0, 0, 0, prefix).expect("repeat cache-free owner edit");
        let owner = &core.document().sections[0].paragraphs[0];
        assert!(
            !owner.line_segs.is_empty(),
            "live layout needs generated rows"
        );
        assert!(
            owner.serializable_line_segs().is_empty(),
            "generated owner cannot become source-backed"
        );
        assert!(core.source_line_cache_snapshot().sections[0].paragraphs[0].line_segs.is_empty());
        let saved = core
            .export_hwp_native()
            .expect("save cache-free owner edit");
        assert!(
            rhwp::parse_document(&saved).unwrap().sections[0].paragraphs[0].line_segs.is_empty(),
            "generated rows cannot enter actual HWP records"
        );
    }
    assert_eq!(
        core.document().sections[0].paragraphs[0].text,
        "Second First Synthetic cache-free owner"
    );
}
