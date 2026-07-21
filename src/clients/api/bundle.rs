use crate::clients::api::client::ApiClient;
use crate::clients::api::error::http_status_error;
use crate::error::GitAiError;
use crate::model::api_types::{CreateBundleRequest, CreateBundleResponse};

/// Bundle API endpoints
impl ApiClient {
    /// Create a new bundle by posting to /api/bundle
    ///
    /// # Arguments
    /// * `request` - The bundle creation request
    ///
    /// # Returns
    /// * `Ok(CreateBundleResponse)` - Success response with bundle ID and URL
    /// * `Err(GitAiError)` - Error response
    ///
    /// # Errors
    /// * Returns `GitAiError::Api` for HTTP errors (use `ApiError::retryability()` to classify)
    /// * Returns `GitAiError::JsonError` for JSON parsing errors
    pub fn create_bundle(
        &self,
        request: CreateBundleRequest,
    ) -> Result<CreateBundleResponse, GitAiError> {
        let response = self.context().post_json("/api/bundles", &request)?;
        let status_code = response.status_code;

        let body = response
            .as_str()
            .map_err(|e| GitAiError::Generic(format!("Failed to read response body: {}", e)))?;

        if status_code == 200 {
            let bundle_response: CreateBundleResponse =
                serde_json::from_str(body).map_err(GitAiError::JsonError)?;
            return Ok(bundle_response);
        }

        Err(http_status_error("bundle create", status_code, body, "unexpected error").into())
    }
}
