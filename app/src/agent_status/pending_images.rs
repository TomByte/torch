use torchui::{Entity, ModelContext};

use super::ImageContext;

/// Images attached to a CLI agent rich input, kept until the prompt is submitted.
#[derive(Default)]
pub struct PendingImages {
    images: Vec<ImageContext>,
}

pub enum PendingImagesEvent {
    Updated,
}

impl PendingImages {
    pub fn images(&self) -> &[ImageContext] {
        &self.images
    }

    pub fn append(&mut self, images: Vec<ImageContext>, ctx: &mut ModelContext<Self>) {
        if images.is_empty() {
            return;
        }
        self.images.extend(images);
        ctx.emit(PendingImagesEvent::Updated);
    }

    pub fn remove(&mut self, index: usize, ctx: &mut ModelContext<Self>) {
        if index < self.images.len() {
            self.images.remove(index);
            ctx.emit(PendingImagesEvent::Updated);
        }
    }

    pub fn take(&mut self, ctx: &mut ModelContext<Self>) -> Vec<ImageContext> {
        let images = std::mem::take(&mut self.images);
        if !images.is_empty() {
            ctx.emit(PendingImagesEvent::Updated);
        }
        images
    }
}

impl Entity for PendingImages {
    type Event = PendingImagesEvent;
}
