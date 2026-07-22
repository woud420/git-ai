use crate::clients::api::client::ApiClient;
use crate::clients::api::error::http_status_error;
use crate::error::GitAiError;
use crate::model::api_types::{CAPromptStoreReadResponse, CasUploadRequest, CasUploadResponse};

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
        let status_code = response.status_code;

        let body = response
            .as_str()
            .map_err(|e| GitAiError::Generic(format!("Failed to read response body: {}", e)))?;

        if status_code == 200 {
            let cas_response: CasUploadResponse =
                serde_json::from_str(body).map_err(GitAiError::JsonError)?;
            return Ok(cas_response);
        }

        Err(http_status_error("CAS upload", status_code, body, "unexpected error").into())
    }

    /// Read CAS objects by hash from the server
    ///
    /// # Arguments
    /// * `hashes` - Slice of CAS hashes to fetch (max 100 per call)
    ///
    /// # Returns
    /// * `Ok(CAPromptStoreReadResponse)` - Response with results for each hash
    /// * `Err(GitAiError)` - On network or server errors
    pub fn read_ca_prompt_store(
        &self,
        hashes: &[&str],
    ) -> Result<CAPromptStoreReadResponse, GitAiError> {
        // Validate all hashes are hex-only before building the URL to prevent
        // injection via crafted hash values in the query string.
        for hash in hashes {
            if !hash.chars().all(|c| c.is_ascii_hexdigit()) {
                return Err(GitAiError::Generic(format!(
                    "CAS hash contains non-hex characters: {}",
                    hash
                )));
            }
        }

        let query = hashes.join(",");
        let endpoint = format!("/worker/cas/?hashes={}", query);
        let response = self.context().get(&endpoint)?;
        let status_code = response.status_code;

        let body = response
            .as_str()
            .map_err(|e| GitAiError::Generic(format!("Failed to read response body: {}", e)))?;

        match status_code {
            200 => {
                let cas_response: CAPromptStoreReadResponse =
                    serde_json::from_str(body).map_err(GitAiError::JsonError)?;
                Ok(cas_response)
            }
            // All hashes not found — return empty response gracefully
            404 => Ok(CAPromptStoreReadResponse {
                results: Vec::new(),
                success_count: 0,
                failure_count: hashes.len(),
            }),
            _ => Err(http_status_error("CAS read", status_code, body, "unexpected error").into()),
        }
    }
}
