use std::error::Error;

use indicatif::{ProgressBar, ProgressStyle};

use crate::core::ExtractOptions;

pub fn extract(options: ExtractOptions) {
    let reporter = Box::new(Reporter::new());
    options.extract(reporter);
}

#[derive(Debug)]
struct Reporter {
    progress_bar: ProgressBar,
}

impl Reporter {
    const PROGRESS_TICKS: usize = 10000;

    fn new() -> Self {
        let style = ProgressStyle::with_template(
            "{prefix:>16!.cyan.bold} [{wide_bar:.white.dim}] {percent:>3.white}%",
        )
        .expect("unable to build progress bar template")
        .progress_chars("=> ");
        let progress_bar = ProgressBar::new(Self::PROGRESS_TICKS as u64)
            .with_prefix("Extracting")
            .with_style(style);
        progress_bar.println("Extracting files...");
        Self { progress_bar }
    }
}

impl crate::core::Reporter for Reporter {
    fn report_progress(&self, progress: f64) {
        let position = progress * Self::PROGRESS_TICKS as f64;
        self.progress_bar.set_position(position as u64);
    }

    fn report_complete(&self) {
        self.progress_bar.finish_and_clear();
        self.progress_bar.println("Extraction complete.");
    }

    fn report_error(&self, error: Box<dyn Error>) {
        self.progress_bar.finish_and_clear();
        let message = format!("Error: {error:?}");
        self.progress_bar.println(message);
    }
}
