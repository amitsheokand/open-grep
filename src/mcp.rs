//! MCP server: `search` + `rg` tools over stdio or loopback HTTP.
//!
//! HTTP binds 127.0.0.1 only and requires `Authorization: Bearer <token>`.

use std::{
    path::{Path, PathBuf},
    sync::OnceLock,
};

use rmcp::{
    ErrorData as McpError, ServiceExt,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{CallToolResult, ContentBlock, ServerCapabilities, ServerInfo},
    schemars, tool, tool_handler, tool_router,
    transport::{
        stdio,
        streamable_http_server::{StreamableHttpService, session::local::LocalSessionManager},
    },
};

use crate::{embed::FastembedProvider, fuse, index, rg};
use serde::Deserialize;

/// Max chars of chunk text per tool hit.
const TEXT_CAP: usize = 1200;

fn check_root(root: &str) -> Result<PathBuf, McpError> {
    let path = PathBuf::from(root);
    if !path.is_absolute() {
        return Err(McpError::invalid_params(
            format!("root must be absolute: {root}"),
            None,
        ));
    }
    if !path.is_dir() {
        return Err(McpError::invalid_params(
            format!("root is not a directory: {root}"),
            None,
        ));
    }
    Ok(path)
}

fn internal(e: impl std::fmt::Display) -> McpError {
    McpError::internal_error(e.to_string(), None)
}

fn render(
    hit_path: &Path,
    start: u64,
    end: u64,
    breadcrumb: &str,
    score: &str,
    text: &str,
) -> String {
    let mut short = text;
    if short.len() > TEXT_CAP {
        let mut end = TEXT_CAP;
        while !short.is_char_boundary(end) {
            end -= 1;
        }
        short = &short[..end];
    }
    format!(
        "{}:{start}-{end} [{breadcrumb}] ({score})\n{short}",
        hit_path.display()
    )
}

fn provider() -> Result<&'static FastembedProvider, McpError> {
    static PROVIDER: OnceLock<Result<FastembedProvider, String>> = OnceLock::new();
    PROVIDER
        .get_or_init(|| FastembedProvider::load().map_err(|e| e.to_string()))
        .as_ref()
        .map_err(|e| McpError::internal_error(format!("embedding model unavailable: {e}"), None))
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct SearchParams {
    /// Absolute workspace root.
    root: String,
    /// Natural-language query; also used for BM25 ranking.
    query: String,
    /// Exact anchors folded into lexical ranking.
    fts: Option<Vec<String>>,
    /// Fuse vector similarity with BM25 (default true).
    fuse: Option<bool>,
    /// Max hits (default 10, max 50).
    limit: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct RgParams {
    /// Absolute workspace root.
    root: String,
    /// Pattern text (literal unless `regex`).
    pattern: String,
    /// Treat pattern as regex.
    regex: Option<bool>,
    /// Case-insensitive matching.
    case_insensitive: Option<bool>,
    /// Max hits (default 100, max 500).
    limit: Option<usize>,
}

#[derive(Clone)]
pub struct OneGrep {
    // Read by `#[tool_handler]` expansion; lint cannot see through the macro.
    #[allow(dead_code)]
    tool_router: ToolRouter<Self>,
}

#[tool_router]
impl OneGrep {
    /// Create the server.
    #[must_use]
    pub fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
        }
    }

    #[tool(
        description = "Hybrid workspace search: semantic discovery fused with BM25, ranked with file:line cites. Falls back to lexical when no vector store exists."
    )]
    async fn search(
        &self,
        Parameters(p): Parameters<SearchParams>,
    ) -> Result<CallToolResult, McpError> {
        let root = check_root(&p.root)?;
        let limit = p.limit.unwrap_or(10).clamp(1, 50);
        let mut query = p.query.clone();
        if let Some(fts) = &p.fts {
            query.push(' ');
            query.push_str(&fts.join(" "));
        }
        let fuse = p.fuse.unwrap_or(true);
        let lines: Vec<String> = if fuse {
            match provider() {
                Ok(provider) => fuse::hybrid(&root, &query, limit, Some(provider), None)
                    .map_err(internal)?
                    .iter()
                    .map(|h| {
                        render(
                            &h.path,
                            h.start,
                            h.end,
                            &h.breadcrumb,
                            &format!("{:.4}", h.score),
                            &h.text,
                        )
                    })
                    .collect(),
                Err(_) => lexical(&root, &query, limit)?,
            }
        } else {
            lexical(&root, &query, limit)?
        };
        Ok(CallToolResult::success(vec![ContentBlock::text(
            lines.join("\n---\n"),
        )]))
    }

    #[tool(
        description = "Exact text or regex search over workspace files (no index needed). Gitignore-aware. Returns path:line:text hits."
    )]
    async fn rg(&self, Parameters(p): Parameters<RgParams>) -> Result<CallToolResult, McpError> {
        let root = check_root(&p.root)?;
        let options = rg::Options {
            regex: p.regex.unwrap_or(false),
            case_insensitive: p.case_insensitive.unwrap_or(false),
            globs: Vec::new(),
            limit: p.limit.unwrap_or(100).clamp(1, 500),
        };
        let lines: Vec<String> = rg::search(&root, &p.pattern, &options)
            .map_err(internal)?
            .iter()
            .map(|h| format!("{}:{}:{}", h.path.display(), h.line, h.text))
            .collect();
        Ok(CallToolResult::success(vec![ContentBlock::text(
            lines.join("\n"),
        )]))
    }
}

impl Default for OneGrep {
    fn default() -> Self {
        Self::new()
    }
}

fn lexical(root: &Path, query: &str, limit: usize) -> Result<Vec<String>, McpError> {
    Ok(index::search(root, query, limit)
        .map_err(internal)?
        .iter()
        .map(|h| {
            render(
                &h.path,
                h.start,
                h.end,
                &h.breadcrumb,
                &format!("{:.2}", h.score),
                &h.text,
            )
        })
        .collect())
}

#[tool_handler]
impl rmcp::ServerHandler for OneGrep {
    fn get_info(&self) -> ServerInfo {
        let mut info = ServerInfo::new(ServerCapabilities::builder().enable_tools().build());
        info.instructions = Some(
            "Local-first hybrid workspace search. Prefer `search` for intent/concepts \
             (it fuses semantic + BM25 ranks); use `rg` to verify exact text, symbols, \
             or regex. Cite path:line evidence."
                .into(),
        );
        info
    }
}

/// Serve over stdio (local MCP clients).
///
/// # Errors
///
/// Returns [`crate::Error`] when transport setup fails.
pub async fn serve_stdio() -> Result<(), crate::Error> {
    OneGrep::new()
        .serve(stdio())
        .await
        .map_err(|e| crate::Error::InvalidInput(e.to_string()))?
        .waiting()
        .await
        .map_err(|e| crate::Error::InvalidInput(e.to_string()))?;
    Ok(())
}

/// Serve Streamable HTTP on 127.0.0.1 with bearer auth.
///
/// # Errors
///
/// Returns [`crate::Error`] when binding or serving fails.
pub async fn serve_http(port: u16, token: &str) -> Result<(), crate::Error> {
    let service = StreamableHttpService::new(
        || Ok(OneGrep::new()),
        LocalSessionManager::default().into(),
        Default::default(),
    );
    let token = token.to_owned();
    let app = axum::Router::new()
        .nest_service("/mcp", service)
        .layer(axum::middleware::from_fn(
            move |req: axum::http::Request<axum::body::Body>, next: axum::middleware::Next| {
                let token = token.clone();
                async move {
                    let ok = req
                        .headers()
                        .get(axum::http::header::AUTHORIZATION)
                        .and_then(|v| v.to_str().ok())
                        == Some(format!("Bearer {token}").as_str());
                    if ok {
                        Ok::<_, (axum::http::StatusCode, String)>(next.run(req).await)
                    } else {
                        Err((
                            axum::http::StatusCode::UNAUTHORIZED,
                            "missing or invalid bearer token".to_owned(),
                        ))
                    }
                }
            },
        ));
    let listener = tokio::net::TcpListener::bind(format!("127.0.0.1:{port}"))
        .await
        .map_err(|e| crate::Error::InvalidInput(e.to_string()))?;
    tracing::info!("serving MCP on 127.0.0.1:{port}/mcp");
    axum::serve(listener, app)
        .await
        .map_err(|e| crate::Error::InvalidInput(e.to_string()))
}
