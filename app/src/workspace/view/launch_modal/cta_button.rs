use super::Slide;
use crate::server::telemetry::TelemetryEvent;

#[derive(Clone)]
pub struct CTAButton<S: Slide> {
    pub label: String,
    pub action: CTAButtonAction<S>,
    #[allow(dead_code)]
    pub telemetry_event: Option<TelemetryEvent>,
}

impl<S: Slide> CTAButton<S> {
    // Constructor methods
    pub fn next_slide(next: S, label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            action: CTAButtonAction::NextSlide(next),
            telemetry_event: None,
        }
    }

    pub fn close(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            action: CTAButtonAction::Close,
            telemetry_event: None,
        }
    }

    #[allow(dead_code)]
    pub fn open_url(label: impl Into<String>, url: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            action: CTAButtonAction::OpenUrl(url.into()),
            telemetry_event: None,
        }
    }

    #[allow(dead_code)]
    pub fn with_telemetry(mut self, event: TelemetryEvent) -> Self {
        self.telemetry_event = Some(event);
        self
    }
}

pub enum CTAButtonAction<S: Slide> {
    NextSlide(S),
    Close,
    #[allow(dead_code)]
    OpenUrl(String),
}

impl<S: Slide> Clone for CTAButtonAction<S> {
    fn clone(&self) -> Self {
        match self {
            CTAButtonAction::NextSlide(s) => CTAButtonAction::NextSlide(*s),
            CTAButtonAction::Close => CTAButtonAction::Close,
            CTAButtonAction::OpenUrl(url) => CTAButtonAction::OpenUrl(url.clone()),
        }
    }
}
