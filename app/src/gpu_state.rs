use torchui::{Entity, SingletonEntity};

/// Singleton model that tracks GPU state.
#[derive(Debug, Default, Clone)]
pub struct GPUState {
    has_low_power_gpu: bool,
}

impl GPUState {
    /// Creates a new GPUState with default values
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns whether the low power GPU is available for use
    pub fn is_low_power_gpu_available(&self) -> bool {
        self.has_low_power_gpu
    }
}

impl SingletonEntity for GPUState {}

impl Entity for GPUState {
    type Event = ();
}
