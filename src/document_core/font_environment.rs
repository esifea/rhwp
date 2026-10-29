use super::DocumentCore;
use crate::model::{
    control::Control,
    identity::walk::{walk, Node},
    paragraph::{LineSeg, Paragraph},
};
use crate::{error::HwpError, renderer::font_environment::FontEnvironment};

fn invalidate_generated_cell_lines(
    paragraphs: &mut [Paragraph],
    environment: Option<&std::sync::Arc<str>>,
) {
    walk(paragraphs, |node| {
        if let Node::Control(Control::Table(table)) = node {
            for cell in &mut table.cells {
                for para in &mut cell.paragraphs {
                    if !para.line_segs.is_empty()
                        && para
                            .line_segs
                            .iter()
                            .all(|line| line.tag & LineSeg::TAG_IMPLEMENTATION_PROPERTY != 0)
                    {
                        para.generated_cell_lines_invalidated =
                            para.generated_cell_lines_environment.as_ref() != environment;
                    }
                }
            }
        }
        Ok(())
    })
    .expect("generated cell-line invalidation is infallible");
}

impl DocumentCore {
    /// 호스트가 실제 프로그램을 준비할 문서 후보와 현재 명시 환경을 반환한다.
    pub fn get_document_font_selection_plan(&self) -> String {
        serde_json::json!({
            "requests": FontEnvironment::document_requests(&self.document.doc_info),
            "environmentId": self.font_environment.as_ref().map(FontEnvironment::id),
            "embeddedFamilies": FontEnvironment::embedded_program_names(&self.document.doc_info),
        })
        .to_string()
    }

    /// 설치 조회 정책을 복제하지 않고 실제 공급 snapshot으로 공통 환경을 계산한다.
    pub fn build_font_environment_from_available_programs(
        &self,
        json: &str,
    ) -> Result<String, HwpError> {
        let environment = FontEnvironment::from_available_programs(&self.document.doc_info, json)?;
        serde_json::to_string(&environment).map_err(|e| HwpError::RenderError(e.to_string()))
    }

    /// Invalidates generated partitions published under a different canonical environment.
    pub(crate) fn invalidate_generated_cell_partitions(&mut self) {
        let environment = self.styles.generated_line_environment.as_ref();
        for section in &mut self.document.sections {
            invalidate_generated_cell_lines(&mut section.paragraphs, environment);
            for master in &mut section.section_def.master_pages {
                invalidate_generated_cell_lines(&mut master.paragraphs, environment);
            }
        }
    }

    /// Change the rendering environment without modifying document fonts or saved lines.
    pub fn set_font_environment(
        &mut self,
        environment: Option<FontEnvironment>,
    ) -> Result<bool, HwpError> {
        if self.batch_mode {
            return Err(HwpError::RenderError(
                "font environment cannot change during an edit batch".into(),
            ));
        }
        if let Some(environment) = &environment {
            environment.validate()?;
        }
        if self.font_environment == environment {
            return Ok(false);
        }
        self.font_environment = environment;
        // Measurements and pending requests from the former font generation must not survive.
        self.canvas_metrics = None;
        self.pending_pagination_job = None;
        self.deferred_pagination_descriptor = None;
        self.rebuild_derived_state();
        Ok(true)
    }

    pub fn font_environment(&self) -> Option<&FontEnvironment> {
        self.font_environment.as_ref()
    }
}
