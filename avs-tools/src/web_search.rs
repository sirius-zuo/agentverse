use agentverse::{Tool, ToolError, ToolResult};
use reqwest::Client;
use schemars::JsonSchema;
use scraper::{Html, Selector};
use serde::Deserialize;
use serde_json::json;
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

#[derive(Deserialize, JsonSchema)]
pub struct WebSearchArgs {
    /// The search query
    pub query: String,
    /// Number of results to fetch and return (1-10)
    pub max_results: u8,
}

#[async_trait::async_trait]
impl Tool for WebSearch {
    type Args = WebSearchArgs;

    fn name(&self) -> &str {
        "web_search"
    }

    fn description(&self) -> &str {
        "Search the web via DuckDuckGo and fetch the content of the top N results"
    }

    async fn execute(&self, args: WebSearchArgs) -> ToolResult {
        let max_results = (args.max_results as usize).clamp(1, 10);

        let html = CLIENT
            .post("https://html.duckduckgo.com/html/")
            .form(&[("q", &args.query)])
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
            results.push(json!({
                "title": title,
                "url": url,
                "snippet": snippet,
                "content": content,
            }));
        }

        Ok(json!(results))
    }
}

/// Parse DDG HTML and return up to `max` results as `(title, url, snippet)`.
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

fn extract_url(href: &str) -> String {
    let full = if href.starts_with("//") {
        format!("https:{href}")
    } else {
        href.to_string()
    };
    let Ok(u) = Url::parse(&full) else {
        return String::new();
    };
    if let Some((_, v)) = u.query_pairs().find(|(k, _)| k == "uddg") {
        return v.into_owned();
    }
    if matches!(u.scheme(), "http" | "https") {
        return full;
    }
    String::new()
}

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
