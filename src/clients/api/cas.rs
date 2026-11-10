use crate::clients::api::client::ApiClient;
use crate::clients::api::response::ResponseEnvelope;
use crate::error::GitAiError;
use crate::model::api_types::{CasUploadRequest, CasUploadResponse};

/// CAS API endpoints
impl ApiClient {
    /// Upload CAS objects to the server
    ///
    /// # Arguments
    /// * `request` - The CAS upload request containing objects to upload
    ///
    /// # Returns
    /// * `Ok(CasUploadResponse)` - Success response
    /// * `Err(GitAiError)` - Error response
    pub fn upload_cas(&self, request: CasUploadRequest) -> Result<CasUploadResponse, GitAiError> {
        let response = self.context().post_json("/worker/cas/upload", &request)?;
        ResponseEnvelope::read(&response)?.decode_json(200, "CAS upload", "unexpected error")
    }
}
