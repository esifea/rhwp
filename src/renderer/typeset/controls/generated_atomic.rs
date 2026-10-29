//! Defers one oversized generated atomic unit without moving its host body.

use super::super::paragraph::metrics::FormattedParagraph;
use super::super::{
    FormattedTable, PageItem, PendingGeneratedAtomicTable, TypesetEngine, TypesetState,
};
use crate::model::{
    control::Control,
    paragraph::Paragraph,
    shape::{HorzAlign, HorzRelTo, VertAlign, VertRelTo},
    table::{Table, TablePageBreak},
};
use crate::renderer::{
    float_placement::{GeneratedAtomicTablePage, ParagraphFloatFlow, ParagraphFloatPlacement},
    height_measurer::MeasuredTable,
    hwpunit_to_px, para_has_no_stored_line_segs,
};

/// Accepts only a complete TABLE row/span closure with no independent subowners.
fn whole_generated_unit(para: &Paragraph, table: &Table, measured: &MeasuredTable) -> bool {
    let rows = usize::from(table.row_count);
    if rows == 0 || measured.row_heights.len() != rows || table.cells.is_empty() {
        return false;
    }
    if table
        .cells
        .iter()
        .any(|cell| cell.row_span == 0 || usize::from(cell.row) + usize::from(cell.row_span) > rows)
    {
        return false;
    }
    let (start, end, _) = measured.row_block_for(0);
    start == 0
        && end == rows
        && para_has_no_stored_line_segs(para)
        && crate::renderer::table_reflows_cell_content(table)
        && para
            .controls
            .iter()
            .filter(|control| {
                matches!(
                    control,
                    Control::Table(_)
                        | Control::Picture(_)
                        | Control::Shape(_)
                        | Control::Equation(_)
                )
            })
            .count()
            == 1
        && table
            .cells
            .iter()
            .all(|cell| cell.paragraphs.iter().all(|para| para.controls.is_empty()))
}

#[allow(clippy::too_many_arguments)]
pub(super) fn defer(
    engine: &TypesetEngine,
    st: &mut TypesetState,
    para_index: usize,
    control_index: usize,
    para: &Paragraph,
    table: &Table,
    ft: &FormattedTable,
    fmt: &FormattedParagraph,
    measured: Option<&MeasuredTable>,
) -> bool {
    let Some(measured) = measured else {
        return false;
    };
    let common = &table.common;
    let top = hwpunit_to_px(i32::from(table.outer_margin_top), engine.dpi);
    let outer_frame_height =
        top + ft.effective_height + hwpunit_to_px(i32::from(table.outer_margin_bottom), engine.dpi);
    if st.profile.hwp5_stored_pagination_layout()
        || st.profile.hwp3_layout()
        || st.col_count != 1
        || st.current_zone_y_offset != 0.0
        || common.treat_as_char
        || table.page_break != TablePageBreak::CellBreak
        || !crate::renderer::float_placement::is_para_topbottom_float(common)
        || !common.flow_with_text
        || common.allow_overlap
        || common.vert_rel_to != VertRelTo::Para
        || common.vert_align != VertAlign::Top
        || common.horz_rel_to != HorzRelTo::Column
        || common.horz_align != HorzAlign::Left
        || table.caption.is_some()
        || !ft.table_footnotes.is_empty()
        || !outer_frame_height.is_finite()
        || outer_frame_height <= st.base_available_height()
        || fmt.line_heights.is_empty()
        || st.current_height + fmt.height_for_fit > st.available_height()
        || !whole_generated_unit(para, table, measured)
    {
        return false;
    }

    // The host owns its ordinary line boxes even when they contain no ink.
    // The table remains absent from this page's placement map and paint items.
    let host_lines = fmt.line_heights.len();
    if host_lines > 0 {
        st.append_item(PageItem::PartialParagraph {
            para_index,
            start_line: 0,
            end_line: host_lines,
        });
        st.advance_flow_by(fmt.total_height);
    }
    st.mark_pre_emitted_host(para_index);
    st.ensure_page();
    let destination_page = st
        .pending_generated_atomic_tables
        .last()
        .map_or(st.pages.len(), |pending| pending.owner.destination_page + 1);
    let owner = GeneratedAtomicTablePage {
        para_index,
        control_index,
        destination_page,
        consumed_end_row: usize::from(table.row_count),
    };
    st.defer_generated_atomic_table(PendingGeneratedAtomicTable {
        owner,
        placement: ParagraphFloatPlacement {
            flow: ParagraphFloatFlow::GeneratedAtomicPage(owner),
            // The destination owns the origin; the anchor's vertical offset is consumed.
            anchor_y: 0.0,
            stored_host_origin: None,
            stored_successor_line_origin: None,
            table_left: None,
            table_top: top,
            occupied_bottom: outer_frame_height,
        },
    });
    true
}
