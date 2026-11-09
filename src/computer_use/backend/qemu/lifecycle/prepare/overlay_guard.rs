use crate::computer_use::guest_image::GuestImageOverlayV1;

pub(super) struct OverlayPreparationGuard(pub(super) Option<GuestImageOverlayV1>);

impl std::ops::Deref for OverlayPreparationGuard {
    type Target = GuestImageOverlayV1;

    fn deref(&self) -> &Self::Target {
        self.0
            .as_ref()
            .expect("overlay remains owned until prepare")
    }
}

impl Drop for OverlayPreparationGuard {
    fn drop(&mut self) {
        if let Some(overlay) = self.0.take() {
            overlay.release_after_vm_stopped().ok();
        }
    }
}
