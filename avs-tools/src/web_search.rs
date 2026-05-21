use agentverse::{AsyncTool, ToolError, ToolResult};
use async_trait::async_trait;
use reqwest::Client;
use scraper::{Html, Selector};
use serde::Serialize;
use serde_json::{json, Value};
use std::sync::LazyLock;
use std::time::Duration;
use url::Url;

static CLIENT: LazyLock<Client> = LazyLock::new(|| {
    Client::builder()
        .timeout(Duration::from_secs(15))
        .user_agent("Mozilla/5.0 (compatible; AgentVerse/0.1)")
        .build()
        .expect("failed to build web search HTTP client")
});

pub struct WebSearch;

#[derive(Serialize)]
struct SearchResult {
    title: String,
    url: String,
    snippet: String,
    content: Option<String>,
}

#[async_trait]
impl AsyncTool for WebSearch {
    fn name(&self) -> &str {
        "web_search"
    }

    fn description(&self) -> &str {
        "Search the web via DuckDuckGo and fetch the content of the top N results"
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "The search query"
                },
                "max_results": {
                    "type": "integer",
                    "description": "Number of results to fetch and return (1-10)"
                }
            },
            "required": ["query", "max_results"]
        })
    }

    async fn execute(&self, args: Value) -> ToolResult {
        let query = args["query"]
            .as_str()
            .ok_or_else(|| ToolError::Execution("Missing 'query' parameter".to_string()))?;
        let max_results = args["max_results"]
            .as_u64()
            .ok_or_else(|| ToolError::Execution("Missing 'max_results' parameter".to_string()))?;
        let max_results = (max_results as usize).clamp(1, 10);

        let html = CLIENT
            .post("https://html.duckduckgo.com/html/")
            .form(&[("q", query)])
            .send()
            .await
            .map_err(|e| ToolError::Execution(format!("DDG search request failed: {e}")))?
            .text()
            .await
            .map_err(|e| ToolError::Execution(format!("DDG response read failed: {e}")))?;

        let candidates = parse_ddg_html(&html, max_results);

        let mut results = Vec::with_capacity(candidates.len());
        for (title, url, snippet) in candidates {
            let content = fetch_page_text(&url).await;
            results.push(SearchResult {
                title,
                url,
                snippet,
                content,
            });
        }

        serde_json::to_value(results)
            .map_err(|e| ToolError::Execution(format!("Serialization failed: {e}")))
    }
}

/// Parse DDG HTML and return up to `max` results as `(title, url, snippet)`.
/// Exported for unit testing.
pub fn parse_ddg_html(html: &str, max: usize) -> Vec<(String, String, String)> {
    let document = Html::parse_document(html);
    let result_sel = Selector::parse("div.result").unwrap();
    let title_sel = Selector::parse("a.result__a").unwrap();
    let snippet_sel = Selector::parse(".result__snippet").unwrap();

    let mut out = Vec::new();
    for result in document.select(&result_sel) {
        if out.len() >= max {
            break;
        }
        let title_el = match result.select(&title_sel).next() {
            Some(el) => el,
            None => continue,
        };
        let title = title_el.text().collect::<String>().trim().to_string();
        if title.is_empty() {
            continue;
        }
        let href = title_el.value().attr("href").unwrap_or("");
        let url = extract_url(href);
        if url.is_empty() {
            continue;
        }
        let snippet = result
            .select(&snippet_sel)
            .next()
            .map(|el| el.text().collect::<String>().trim().to_string())
            .unwrap_or_default();

        out.push((title, url, snippet));
    }
    out
}

/// Decode the actual destination URL from a DDG redirect href.
/// DDG wraps result URLs as `//duckduckgo.com/l/?uddg=<url-encoded-url>&rut=...`
fn extract_url(href: &str) -> String {
    let full = if href.starts_with("//") {
        format!("https:{href}")
    } else {
        href.to_string()
    };
    Url::parse(&full)
        .ok()
        .and_then(|u| {
            u.query_pairs()
                .find(|(k, _)| k == "uddg")
                .map(|(_, v)| v.into_owned())
        })
        .unwrap_or_default()
}

/// Fetch a URL and extract readable text from `<p>` tags, capped at 2000 chars.
async fn fetch_page_text(url: &str) -> Option<String> {
    let html = CLIENT.get(url).send().await.ok()?.text().await.ok()?;
    let document = Html::parse_document(&html);
    let p_sel = Selector::parse("p").unwrap();
    let text: String = document
        .select(&p_sel)
        .map(|el| el.text().collect::<String>())
        .filter(|t| !t.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    if text.is_empty() {
        None
    } else {
        Some(text.chars().take(2000).collect())
    }
}
