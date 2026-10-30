//! HWP5 구역 끝 목록의 저장 소유. 적용/렌더링 플래그와 저장 위치는 별개다.

use std::collections::{BTreeMap, VecDeque};

use super::control::{Control, Field};
use super::document::{Document, RawRecord, Section};
use super::identity::walk::{walk_ref, RefNode};

/// 미해석 정보까지 포함하는 원본 목록. 본문 편집으로 이 범위를 버리지 않는다.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct SectionTail {
    pub records: Vec<RawRecord>,
    /// 로드 후 모델 정규화가 끝난 시점의 바탕쪽·메모 참조 투영.
    #[serde(skip)]
    pub(crate) projection_seal: Option<[u8; 32]>,
    /// Unsupported master-page edits must not replay stale independent lists.
    #[serde(skip)]
    pub(crate) master_projection_seal: Option<[u8; 32]>,
    /// Retain the section parser's master projection before document attachment.
    #[serde(skip)]
    pub(crate) local_master_projection_seal: Option<[u8; 32]>,
    #[serde(skip)]
    pub(crate) local_memo_projection_seal: Option<[u8; 32]>,
    /// Record offsets owned by this section; other sections' memo spans stay raw.
    #[serde(skip)]
    pub(crate) local_memo_spans: Option<Vec<usize>>,
    /// Retains authenticated source owners for each native list after visibility.
    /// `None` entries belong to hidden owners and remain only in review history.
    #[serde(skip)]
    pub(crate) source_memo_owners: Option<Vec<Option<usize>>>,
}

fn memo_fields(section: &Section) -> Vec<&Field> {
    let mut memos = Vec::new();
    let result = walk_ref(&section.paragraphs, &crate::storage::Unbounded, |node| {
        if let RefNode::Control(Control::Field(field)) = node {
            if field.is_memo() {
                memos.push(field);
            }
        }
        Ok(())
    });
    match result {
        Ok(()) => memos,
        Err(never) => match never {},
    }
}

pub(crate) fn local_memo_projection_digest(section: &Section) -> [u8; 32] {
    crate::model::raw_provenance::value_digest(
        &memo_fields(section),
        "메모 모델은 메모리에서 직렬화한다",
    )
}

pub(crate) fn local_master_projection_digest(section: &Section) -> [u8; 32] {
    crate::model::raw_provenance::value_digest(
        &section.section_def.master_pages,
        "바탕쪽 모델은 메모리에서 직렬화한다",
    )
}

/// 본문 글자/캐럿 위치는 제외하고, 원본 목록의 의미·참조를 바꾸는 모델만 봉인한다.
/// 메모 목록은 문서 단위로 저장하되, 소유 구역 경계도 봉인해 구역 이동을 감지한다.
pub(crate) fn projection_digest(doc: &Document) -> [u8; 32] {
    let masters: Vec<_> = doc
        .sections
        .iter()
        .map(|s| &s.section_def.master_pages)
        .collect();
    let memos: Vec<_> = doc.sections.iter().map(memo_fields).collect();
    crate::model::raw_provenance::value_digest(
        &(masters, memos),
        "구역 끝 모델은 메모리에서 직렬화한다",
    )
}

/// Seals FIFO memo ownership after all sections have attached their native lists.
pub(crate) fn seal_projections(doc: &mut Document) {
    if !doc.sections.iter().any(|s| s.trailing_lists.is_some()) {
        return;
    }
    let digest = projection_digest(doc);
    let master_digest = master_projection_digest(doc);
    let local_digests: Vec<_> = doc
        .sections
        .iter()
        .map(local_memo_projection_digest)
        .collect();
    let mut owners: BTreeMap<u32, VecDeque<usize>> = BTreeMap::new();
    for (index, section) in doc.sections.iter().enumerate() {
        for field in memo_fields(section) {
            owners
                .entry(field.hwp_memo_index())
                .or_default()
                .push_back(index);
        }
    }
    for (section_index, section) in doc.sections.iter_mut().enumerate() {
        if let Some(tail) = &mut section.trailing_lists {
            let mut local_spans = Vec::new();
            let mut source_owners = tail
                .source_memo_owners
                .as_deref()
                .map(|owners| owners.iter());
            for (offset, record) in tail.records.iter().enumerate() {
                if record.tag_id != crate::parser::tags::HWPTAG_MEMO_LIST || record.level != 1 {
                    continue;
                }
                if let Some(index) = record.data.get(..4) {
                    let index = u32::from_le_bytes(index.try_into().unwrap());
                    let owner = match &mut source_owners {
                        Some(source) => source.next().copied().flatten(),
                        None => owners.get_mut(&index).and_then(VecDeque::pop_front),
                    };
                    if owner == Some(section_index) {
                        local_spans.push(offset);
                    }
                }
            }
            tail.projection_seal = Some(digest);
            tail.master_projection_seal = Some(master_digest);
            tail.local_memo_projection_seal = Some(local_digests[section_index]);
            tail.local_memo_spans = Some(local_spans);
        }
    }
}

pub(crate) fn master_projection_digest(doc: &Document) -> [u8; 32] {
    let masters: Vec<_> = doc
        .sections
        .iter()
        .map(|s| &s.section_def.master_pages)
        .collect();
    crate::model::raw_provenance::value_digest(&masters, "바탕쪽 모델 투영은 메모리에서 직렬화한다")
}

pub(crate) fn memo_projection_changed(doc: &Document) -> bool {
    let digest = projection_digest(doc);
    doc.sections
        .iter()
        .filter_map(|s| s.trailing_lists.as_ref())
        .any(|tail| tail.projection_seal != Some(digest))
}

pub(crate) fn validate_preservation(doc: &Document) -> Result<(), String> {
    if !doc.sections.iter().any(|s| s.trailing_lists.is_some()) {
        return Ok(());
    }
    let digest = master_projection_digest(doc);
    for (index, section) in doc.sections.iter().enumerate() {
        if let Some(tail) = &section.trailing_lists {
            let unchanged = match tail.master_projection_seal {
                Some(seal) => seal == digest,
                None => {
                    tail.local_master_projection_seal
                        == Some(local_master_projection_digest(section))
                }
            };
            if !unchanged {
                return Err(format!("구역 {index}의 원본 바탕쪽 목록과 현재 모델의 참조가 달라졌습니다. 바탕쪽 목록 자체의 재작성은 아직 지원하지 않습니다."));
            }
        }
    }
    Ok(())
}
