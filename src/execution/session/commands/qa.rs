use crate::execution::errors::ExecutionError;

use super::super::{ExecutionSession, policy_error};

impl ExecutionSession {
    /// Launch one app-owned QA driver through the same selected execution
    /// backend and only when this session was created for Runtime QA.
    pub fn run_qa_driver(
        &self,
        container_id: &str,
        spec: &crate::execution::docker::qa_container::QaDriverContainerSpec<'_>,
        cancel: &mut tokio::sync::watch::Receiver<bool>,
    ) -> Result<crate::execution::docker::qa_container::DriverContainerOutput, ExecutionError> {
        self.verify_authorization()?;
        if self.mode != crate::execution::contract::ExecutionModeV1::RuntimeQa
            || spec.image.digest != self.selected_image_digest()?
        {
            return Err(policy_error(
                "Runtime QA driver does not match the selected QA session and image",
            ));
        }
        let result = self
            .runtime
            .block_on(self.backend.run_qa_driver(container_id, spec, cancel));
        self.verify_authorization()?;
        result
    }

    pub fn create_qa_driver(
        &self,
        spec: &crate::execution::docker::qa_container::QaDriverContainerSpec<'_>,
        uploads: &[crate::execution::docker::qa_container::QaContainerUpload],
        cancel: &mut tokio::sync::watch::Receiver<bool>,
    ) -> Result<String, ExecutionError> {
        self.verify_authorization()?;
        if self.mode != crate::execution::contract::ExecutionModeV1::RuntimeQa
            || spec.image.digest != self.selected_image_digest()?
        {
            return Err(policy_error(
                "Runtime QA driver does not match the selected QA session and image",
            ));
        }
        self.runtime
            .block_on(self.backend.create_qa_driver(spec, uploads, cancel))
    }

    pub fn cleanup_qa_driver(&self, container_id: &str) -> Result<(), ExecutionError> {
        self.runtime
            .block_on(self.backend.cleanup_qa_driver(container_id))
    }

    pub fn start_qa_web_preview(
        &self,
        spec: crate::execution::docker::qa_container::QaWebPreviewSpec<'_>,
        cancel: &mut tokio::sync::watch::Receiver<bool>,
    ) -> Result<crate::execution::docker::qa_container::QaWebPreview, ExecutionError> {
        self.verify_authorization()?;
        if self.mode != crate::execution::contract::ExecutionModeV1::RuntimeQa
            || spec.browser_image.digest != self.selected_image_digest()?
        {
            return Err(policy_error(
                "Runtime QA web preview does not match the selected browser session",
            ));
        }
        let result = self
            .runtime
            .block_on(self.backend.start_qa_web_preview(spec, cancel));
        self.verify_authorization()?;
        result
    }

    pub fn cleanup_qa_web_preview(
        &self,
        lease: &crate::runtime_qa::QaTaskServiceLeaseV1,
        binding: &crate::runtime_qa::QaPreviewRelayBindingV1,
    ) -> Result<(), ExecutionError> {
        self.runtime
            .block_on(self.backend.cleanup_qa_web_preview(lease, binding))
    }

    pub fn recover_qa_web_preview(
        &self,
        request: &crate::runtime_qa::QaRunRequestV1,
    ) -> Result<(), ExecutionError> {
        self.verify_authorization()?;
        if self.mode != crate::execution::contract::ExecutionModeV1::RuntimeQa {
            return Err(policy_error(
                "QA preview recovery requires a Runtime QA session",
            ));
        }
        let result = self
            .runtime
            .block_on(self.backend.recover_qa_web_preview(request));
        self.verify_authorization()?;
        result
    }
}
