//! 일반 TAC 표 문단의 배치 전 수용 판단.
//! 줄 상자·실측·저장 경계의 기존 선택 순서를 보존하고 상태 쓰기는 조정자에 맡긴다.

use super::super::paragraph::metrics::FormattedParagraph;
use super::super::{
    line_seg_visible_bounds_px, saved_table_bounds_fit_at_flow_tail, stored_tac_table_frame_height,
};
use super::tac_flow::TacFlowQuery;
use crate::model::{
    control::Control, paragraph::Paragraph, provenance::LayoutCompatibilityProfile,
};
use crate::renderer::{height_measurer::MeasuredTable, hwpunit_to_px, pagination::PageItem};

pub(in crate::renderer::typeset) struct TacFitPage<'a> {
    pub profile: LayoutCompatibilityProfile,
    pub current_height: f64,
    pub vpos_page_base: Option<i32>,
    pub current_items: &'a [PageItem],
}

pub(in crate::renderer::typeset) struct TacFitPlan {
    pub tac_count: usize,
    pub has_tac: bool,
    // 뒤쪽 저장 높이 cap도 동일한 편집 후 실측 결과를 소비한다.
    pub measured_tac_floor: Option<f64>,
    pub advance_before_place: bool,
}

/// Owner-line mapping for fit and table placement.
pub(in crate::renderer::typeset) fn table_line_index(
    para: &Paragraph,
    table: &crate::model::table::Table,
    control_index: usize,
    tac_count: usize,
    fmt: &FormattedParagraph,
    flow: &TacFlowQuery<'_>,
) -> usize {
    if tac_count <= 1 {
        return flow.tac_table_line_index(para, table, fmt).unwrap_or(0);
    }

    // Multiple tables follow the leading table's owner line.
    let leading = para
        .controls
        .iter()
        .find_map(|control| {
            let Control::Table(table) = control else {
                return None;
            };
            flow.is_effective_tac_table(para, table, fmt)
                .then(|| flow.tac_table_line_index(para, table, fmt))
        })
        .flatten()
        .unwrap_or(0);
    let prior = para
        .controls
        .iter()
        .take(control_index)
        .filter(|control| {
            matches!(control, Control::Table(table)
            if flow.is_effective_tac_table(para, table, fmt))
        })
        .count();

    // Final offset
    leading + prior
}

/// 가용 높이(진단 포함)는 원래 저장 경계/최종 fit 위치에서만 조회한다.
pub(super) fn prepare(
    para_idx: usize,
    para: &Paragraph,
    fmt: &FormattedParagraph,
    measured_tables: &[MeasuredTable],
    page: TacFitPage<'_>,
    available_height: impl Fn() -> f64,
    flow: TacFlowQuery<'_>,
) -> TacFitPlan {
    let dpi = flow.dpi();
    // TAC 표 카운트 및 플러시 판단
    let tac_count = para
        .controls
        .iter()
        .filter(|c| matches!(c, Control::Table(t) if flow.is_effective_tac_table(para, t, fmt)))
        .count();

    let has_tac = tac_count > 0;
    let first_line_tac_height = if tac_count == 1 && fmt.line_heights.len() > 1 {
        para.controls.iter().find_map(|ctrl| match ctrl {
            Control::Table(t)
                if flow.is_effective_tac_table(para, t, fmt)
                    && flow.tac_table_line_index(para, t, fmt) == Some(0) =>
            {
                Some(
                    fmt.line_heights
                        .first()
                        .copied()
                        .unwrap_or_else(|| fmt.line_advance(0)),
                )
            }
            _ => None,
        })
    } else {
        None
    };
    // Reserve each grown table's owner frame from the measured row body.
    let mut grown_line_heights = fmt.line_heights.clone();
    let mut has_grown_table = false;
    for (ci, ctrl) in para.controls.iter().enumerate() {
        let Control::Table(table) = ctrl else {
            continue;
        };
        if !flow.is_effective_tac_table(para, table, fmt) {
            continue;
        }

        let reflows = !page.profile.native_hwp5_layout()
            && crate::renderer::table_reflows_cell_content(table);
        let unstored_text =
            flow.single_tac_line_has_unstored_cell_text(para, table, fmt, tac_count);
        let declared = hwpunit_to_px(table.common.height as i32, dpi);
        let Some(measured) = measured_tables
            .iter()
            .find(|m| m.para_index == para_idx && m.control_index == ci)
            .filter(|m| {
                let body = crate::renderer::table_row_body_height(&m.cumulative_heights);
                ((reflows || (unstored_text && !flow.session_edited()))
                    && body > declared + 0.5)
                    || (flow.session_edited() && body > declared + 8.0)
            })
        else {
            continue;
        };
        has_grown_table = true;

        let line_index = table_line_index(para, table, ci, tac_count, fmt, &flow);
        if grown_line_heights.len() <= line_index {
            grown_line_heights.resize(line_index + 1, 0.0);
        }
        // Get final frame by table height + top/bottom outer_margin
        let frame = measured.total_height
            + hwpunit_to_px(table.outer_margin_top as i32, dpi)
            + hwpunit_to_px(table.outer_margin_bottom as i32, dpi);
        grown_line_heights[line_index] = grown_line_heights[line_index].max(frame);
    }
    // Reserve host line frames using the placement path owner mapping
    let measured_tac_floor = has_grown_table.then(|| {
        grown_line_heights.iter().sum::<f64>()
            + fmt.line_spacings.iter().sum::<f64>()
            + fmt.spacing_before
            + fmt.spacing_after
    });
    // 실제 TAC 배치가 사용하는 소유 줄 상자는 바깥여백을 이미 포함한다.
    // pre-flush에서 fmt와 여백을 다시 더하면 실제로 들어가는 표를 먼저 이월한다.
    let owned_single_tac_frame = (page.profile.hwpx_stored_layout()
        && tac_count == 1
        && fmt.line_heights.len() == 1)
        .then(|| {
            para.controls.iter().enumerate().find_map(|(ci, control)| {
                let Control::Table(table) = control else {
                    return None;
                };
                crate::renderer::composer::owned_rowbreak_tac_height(para, ci).filter(|height| {
                    i64::from(*height)
                        >= i64::from(table.common.height)
                            + i64::from(table.outer_margin_top)
                            + i64::from(table.outer_margin_bottom)
                })
            })
        })
        .flatten()
        .map(|height| hwpunit_to_px(height, dpi));
    let height_for_fit = if let Some(height) = owned_single_tac_frame {
        let base = height + fmt.spacing_before;
        measured_tac_floor.map_or(base, |grown| base.max(grown))
    } else if has_tac {
        // 글자처럼 취급되는 표는 **바깥 여백(위·아래)까지 쪽 예산을 차지**한다.
        // 한컴 저장 lineseg 의 vertsize 가 `표 선언높이 + outMargin.top + outMargin.bottom`
        // 이다(본 문서 TAC 개체 18/18 일치, 2248+283+283=2814). 이 항이 빠져 쪽마다
        // 566 HU 씩 덜 쌓였고, 소제목 표가 앞 쪽 바닥에 남아 이후 쪽이 통째로 밀렸다.
        // 소유 줄이 상하 여백까지 담는 경로는 위에서 한 번만 계상한다.
        // 그 증거가 없는 저장 줄은 기존 수용 판정의 여백 보충을 유지한다.
        let tac_outer_margin_px: f64 = para
            .controls
            .iter()
            .filter_map(|ctrl| match ctrl {
                Control::Table(t) if flow.is_effective_tac_table(para, t, fmt) => {
                    Some(crate::renderer::hwpunit_to_px(
                        i32::from(t.common.margin.top) + i32::from(t.common.margin.bottom),
                        dpi,
                    ))
                }
                _ => None,
            })
            .fold(0.0f64, f64::max);
        let base = first_line_tac_height.unwrap_or(fmt.height_for_fit) + tac_outer_margin_px;
        measured_tac_floor.map_or(base, |grown| base.max(grown))
    } else {
        fmt.total_height
    };
    let saved_single_tac_bottom_fits = if has_tac && tac_count <= 1 && measured_tac_floor.is_none() {
        para.controls
            .iter()
            .find_map(|ctrl| match ctrl {
                Control::Table(table) if flow.is_effective_tac_table(para, table, fmt) => Some((
                    flow.tac_table_line_index(para, table, fmt).unwrap_or(0),
                    stored_tac_table_frame_height(table, dpi, height_for_fit),
                )),
                _ => None,
            })
            .and_then(|(line_idx, frame_height)| {
                para.line_segs.get(line_idx).and_then(|seg| {
                    line_seg_visible_bounds_px(seg, page.vpos_page_base.unwrap_or(0), dpi)
                        .map(|bounds| (bounds, frame_height))
                })
            })
            .is_some_and(|(bounds, frame_height)| {
                saved_table_bounds_fit_at_flow_tail(
                    bounds,
                    page.current_height,
                    available_height(),
                    frame_height,
                )
            })
    } else {
        false
    };
    // [#2311] 단일 TAC 표가 후행 줄(ctrl 1:1 lineseg, vpos==0 저장 리셋)에 있고
    // 선행 줄이 전부 TAC 그림/도형이면, 표는 아래 #1152 intra-para reset 가드가
    // 자체적으로 새 쪽 이동한다. 이때 pre-flush 를 문단 전체 높이로 판정하면
    // 잔여 공간에 들어가는 선행 전면 그림까지 통째로 밀려 한글 대비 +1쪽씩
    // 벌어진다 (10k r15 156744475: 붙임 포스터+차기 붙임 헤더 표 문단 ×2 →
    // rhwp 5쪽 vs 한글 3쪽, 저장 ls[0] vpos=5435 는 같은 쪽 배치를 명시).
    // 리셋 이전 줄들의 높이만 fit 기준으로 삼는다.
    let pre_reset_height_for_fit = if has_tac
        && tac_count == 1
        && first_line_tac_height.is_none()
        && para.text.is_empty()
        && para.line_segs.len() == para.controls.len()
    {
        para.controls
            .iter()
            .position(
                |c| matches!(c, Control::Table(t) if flow.is_effective_tac_table(para, t, fmt)),
            )
            .filter(|&ti| {
                ti > 0
                    && ti <= fmt.line_heights.len()
                    && para.line_segs.get(ti).map(|s| s.vertical_pos) == Some(0)
                    && para.controls[..ti].iter().all(|c| match c {
                        Control::Picture(p) => p.common.treat_as_char,
                        Control::Shape(s) => s.common().treat_as_char,
                        _ => false,
                    })
            })
            .map(|ti| (0..ti).map(|li| fmt.line_advance(li)).sum::<f64>())
    } else {
        None
    };
    let height_for_fit = pre_reset_height_for_fit.unwrap_or(height_for_fit);

    // 넘치면 flush (단일 TAC 표만)
    let advance_before_place = page.current_height + height_for_fit > available_height()
        && !page.current_items.is_empty()
        && has_tac
        && tac_count <= 1
        && !saved_single_tac_bottom_fits;
    TacFitPlan {
        tac_count,
        has_tac,
        measured_tac_floor,
        advance_before_place,
    }
}
