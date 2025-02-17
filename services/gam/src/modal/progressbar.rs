use blitstr2::GlyphStyle;

use crate::*;

/// This is an extention to the Slider struct that allows it to be used as a progress bar
pub struct ProgressBar<'a, 'b> {
    // work is the measure of the actual work being done (e.g. sectors to erase start/end)
    subtask_start_work: u32,
    subtask_end_work: u32,
    // this is the value of the work that's been done
    current_work: u32,
    // percent is the start/end percentage points of the overall 100% range this subtask maps to
    subtask_start_percent: u32,
    subtask_end_percent: u32,
    // this is the absolute value of the current progress in percent
    current_progress_percent: u32,
    modal: &'a mut Modal<'b>,
    slider: &'a mut Slider,
}
impl<'a, 'b> ProgressBar<'a, 'b> {
    pub fn new(modal: &'a mut Modal<'b>, slider: &'a mut Slider) -> ProgressBar<'a, 'b> {
        ProgressBar {
            subtask_start_work: 0,
            subtask_end_work: 255,
            current_work: 0,
            subtask_start_percent: 0,
            subtask_end_percent: 100,
            current_progress_percent: 0,
            modal,
            slider,
        }
    }

    pub fn modify(
        &mut self,
        update_action: Option<ActionType>,
        update_top_text: Option<&str>,
        remove_top: bool,
        update_bot_text: Option<&str>,
        remove_bot: bool,
        update_style: Option<GlyphStyle>,
    ) {
        self.modal.modify(
            update_action,
            update_top_text,
            remove_top,
            update_bot_text,
            remove_bot,
            update_style,
        );
    }

    pub fn activate(&self) { self.modal.activate(); }

    pub fn update_text(&mut self, text: &str) {
        self.modal.modify(None, Some(text), false, None, false, None);
    }

    /// There is a significant caveat for performance/stability for this routine.
    /// This works well for a modal that is both managed and owned by the same thread so long as
    /// one is not resizing the top or bottom text within the modal while changing the progress bar
    /// However, if one triggers a redraw/resize of the text, then the system needs to re-run the
    /// defacement computations on the underlying canvases. If one or two calls happen, this might
    /// not be a big deal, but because the redraw requests that are looped back from the GAM as a
    /// result of the defacement operation cannot be processed until the thread managing the progress
    /// bar has finished and returned control to the main loop, redraw requests will eventually fill
    /// up the server queue and cause a deadlock situation.
    fn update_ui(&mut self, new_percent: u32) {
        if new_percent != self.current_progress_percent {
            log::debug!("progress: {}", new_percent);
            self.slider.set_state(new_percent);
            self.modal.modify(
                Some(crate::ActionType::Slider(self.slider.clone())),
                None,
                false,
                None,
                false,
                None,
            );
            self.modal.redraw(); // stage the modal box pixels to the back buffer
            xous::yield_slice(); // this gives time for the GAM to do the sending
            self.current_progress_percent = new_percent;
        }
    }

    pub fn increment_work(&mut self, increment: u32) {
        self.current_work += increment;
        if self.current_work > self.subtask_end_work {
            self.current_work = self.subtask_end_work;
        }
        let new_progress_percent = self.subtask_start_percent
            + ((self.subtask_end_percent - self.subtask_start_percent) * self.current_work)
                / (self.subtask_end_work - self.subtask_start_work);
        self.update_ui(new_progress_percent);
    }

    pub fn set_percentage(&mut self, setting: u32) {
        let checked_setting = if setting > 100 { 100 } else { setting };
        self.update_ui(checked_setting);
    }

    pub fn rebase_subtask_work(&mut self, subtask_start_work: u32, subtask_end_work: u32) {
        assert!(subtask_end_work > subtask_start_work);
        self.subtask_start_work = subtask_start_work;
        self.subtask_end_work = subtask_end_work;
        self.current_work = self.subtask_start_work;
        self.increment_work(0); // this will recompute the Ux state and draw it
    }

    pub fn rebase_subtask_percentage(&mut self, subtask_start_percent: u32, subtask_end_percent: u32) {
        assert!(subtask_start_percent <= subtask_end_percent);
        self.subtask_start_percent = subtask_start_percent;
        self.subtask_end_percent = subtask_end_percent;
        self.update_ui(self.subtask_start_percent); // this will redraw the UI if the start percent is different from the current
    }
}
