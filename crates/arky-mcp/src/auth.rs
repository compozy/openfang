//! Authentication helpers for HTTP-based MCP servers.

use std::fmt;

use reqwest::Client as HttpClient;
use rmcp::transport::{AuthClient, AuthError, auth::OAuthState};

use crate::McpError;

/// Authentication configuration for HTTP MCP clients.
#[derive(Clone)]
pub enum McpAuth {
    /// Static bearer token authentication.
    BearerToken(String),
    /// Fully authorized OAuth client.
    OAuth(McpOAuthAuth),
}

impl fmt::Debug for McpAuth {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BearerToken(_) => f.write_str("McpAuth::BearerToken(..)"),
            Self::OAuth(_) => f.write_str("McpAuth::OAuth(..)"),
        }
    }
}

/// Cloneable authorized OAuth client that can be plugged into rmcp transports.
#[derive(Clone)]
pub struct McpOAuthAuth {
    client: AuthClient<HttpClient>,
}

impl fmt::Debug for McpOAuthAuth {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("McpOAuthAuth(..)")
    }
}

impl McpOAuthAuth {
    const fn new(client: AuthClient<HttpClient>) -> Self {
        Self { client }
    }

    pub(crate) fn client(&self) -> AuthClient<HttpClient> {
        self.client.clone()
    }
}

/// Configuration for an interactive OAuth flow.
#[derive(Debug, Clone)]
pub struct McpOAuthOptions {
    /// Redirect URI the OAuth provider should return to.
    pub redirect_uri: String,
    /// Requested scopes. Empty means “let the server choose”.
    pub scopes: Vec<String>,
    /// Optional public client name used during dynamic registration.
    pub client_name: Option<String>,
    /// Optional client metadata URL for SEP-991 flows.
    pub client_metadata_url: Option<String>,
}

impl McpOAuthOptions {
    /// Creates a new OAuth-options value with a redirect URI.
    #[must_use]
    pub fn new(redirect_uri: impl Into<String>) -> Self {
        Self {
            redirect_uri: redirect_uri.into(),
            scopes: Vec::new(),
            client_name: None,
            client_metadata_url: None,
        }
    }
}

/// Interactive OAuth flow wrapper around rmcp's `OAuthState`.
pub struct McpOAuthFlow {
    state: OAuthState,
    options: McpOAuthOptions,
}

impl fmt::Debug for McpOAuthFlow {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("McpOAuthFlow")
            .field("options", &self.options)
            .finish_non_exhaustive()
    }
}

impl McpOAuthFlow {
    /// Creates a new OAuth flow for a protected MCP resource.
    pub async fn new(
        server_url: impl AsRef<str>,
        options: McpOAuthOptions,
    ) -> Result<Self, McpError> {
        let state = OAuthState::new(server_url.as_ref(), Some(HttpClient::new()))
            .await
            .map_err(|error| map_auth_error(&error))?;

        Ok(Self { state, options })
    }

    /// Starts authorization if needed and returns the URL the user should open.
    pub async fn authorization_url(&mut self) -> Result<String, McpError> {
        let scopes = self
            .options
            .scopes
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>();

        self.state
            .start_authorization_with_metadata_url(
                &scopes,
                &self.options.redirect_uri,
                self.options.client_name.as_deref(),
                self.options.client_metadata_url.as_deref(),
            )
            .await
            .map_err(|error| map_auth_error(&error))?;

        self.state
            .get_authorization_url()
            .await
            .map_err(|error| map_auth_error(&error))
    }

    /// Completes the OAuth callback step with the returned code and CSRF token.
    pub async fn complete_authorization(
        &mut self,
        code: &str,
        csrf_token: &str,
    ) -> Result<(), McpError> {
        self.state
            .handle_callback(code, csrf_token)
            .await
            .map_err(|error| map_auth_error(&error))
    }

    /// Consumes the interactive flow and returns a reusable auth object.
    pub fn into_auth(self) -> Result<McpOAuthAuth, McpError> {
        match self.state {
            OAuthState::Authorized(manager) => Ok(McpOAuthAuth::new(AuthClient::new(
                HttpClient::new(),
                manager,
            ))),
            _ => Err(McpError::auth_failed(
                "OAuth flow is not yet authorized",
                None,
            )),
        }
    }
}

fn map_auth_error(error: &AuthError) -> McpError {
    McpError::auth_failed(
        error.to_string(),
        Some(serde_json::json!({
            "auth_error": format!("{error:?}"),
        })),
    )
}
