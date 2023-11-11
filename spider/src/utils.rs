use cowstr::CowStr;
use log::{info, log_enabled, Level};
use reqwest::Client;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

#[cfg(all(not(feature = "fs"), feature = "chrome"))]
/// Perform a network request to a resource extracting all content as text streaming via chrome.
pub async fn fetch_page_html(
    target_url: &str,
    client: &Client,
    page: &chromiumoxide::Page,
) -> (Option<bytes::Bytes>, Option<String>) {
    match page.goto(target_url).await {
        Ok(page) => {
            let p = page.wait_for_navigation_response().await;
            let res = page.content().await;

            (
                Some(res.unwrap_or_default().into()),
                match p {
                    Ok(u) => get_last_redirect(&target_url, &u),
                    _ => None,
                },
            )
        }
        _ => fetch_page_html_raw(&target_url, &client).await,
    }
}

#[cfg(all(not(feature = "fs"), feature = "chrome"))]
/// Check if url matches the last item in a redirect chain for chrome CDP
pub fn get_last_redirect(
    target_url: &str,
    u: &Option<std::sync::Arc<chromiumoxide::handler::http::HttpRequest>>,
) -> Option<String> {
    match u {
        Some(u) => match u.redirect_chain.last()? {
            r => match r.url.as_ref()? {
                u => {
                    if target_url != u {
                        Some(u.into())
                    } else {
                        None
                    }
                }
            },
        },
        _ => None,
    }
}

/// Perform a network request to a resource extracting all content streaming.
pub async fn fetch_page_html_raw(
    target_url: &str,
    client: &Client,
) -> (Option<bytes::Bytes>, Option<String>) {
    use crate::bytes::BufMut;
    use bytes::BytesMut;
    use tokio_stream::StreamExt;

    match client.get(target_url).send().await {
        Ok(res) if res.status().is_success() => {
            let u = res.url().as_str();

            let rd = if target_url != u {
                Some(u.into())
            } else {
                None
            };

            let mut stream = res.bytes_stream();
            let mut data: BytesMut = BytesMut::new();

            while let Some(item) = stream.next().await {
                match item {
                    Ok(text) => data.put(text),
                    _ => (),
                }
            }

            (Some(data.into()), rd)
        }
        Ok(_) => Default::default(),
        Err(_) => {
            log("- error parsing html text {}", &target_url);
            Default::default()
        }
    }
}

#[cfg(all(not(feature = "fs"), not(feature = "chrome")))]
/// Perform a network request to a resource extracting all content as text streaming.
pub async fn fetch_page_html(
    target_url: &str,
    client: &Client,
) -> (Option<bytes::Bytes>, Option<String>) {
    fetch_page_html_raw(&target_url, &client).await
}

/// Perform a network request to a resource extracting all content as text.
#[cfg(feature = "decentralized")]
pub async fn fetch_page(target_url: &str, client: &Client) -> Option<bytes::Bytes> {
    match client.get(target_url).send().await {
        Ok(res) if res.status().is_success() => match res.bytes().await {
            Ok(text) => Some(text),
            Err(_) => {
                log("- error fetching {}", &target_url);
                None
            }
        },
        Ok(_) => None,
        Err(_) => {
            log("- error parsing html bytes {}", &target_url);
            None
        }
    }
}

/// Perform a network request to a resource extracting all content as text streaming.
#[cfg(feature = "fs")]
pub async fn fetch_page_html(
    target_url: &str,
    client: &Client,
) -> (Option<bytes::Bytes>, Option<String>) {
    use crate::bytes::BufMut;
    use crate::tokio::io::AsyncReadExt;
    use crate::tokio::io::AsyncWriteExt;
    use bytes::BytesMut;
    use percent_encoding::utf8_percent_encode;
    use percent_encoding::NON_ALPHANUMERIC;
    use std::time::SystemTime;
    use tendril::fmt::Slice;
    use tokio_stream::StreamExt;

    lazy_static! {
        static ref TMP_DIR: String = {
            use std::fs;
            let mut tmp = std::env::temp_dir();

            tmp.push("spider/");

            // make sure spider dir is created.
            match fs::create_dir_all(&tmp) {
                Ok(_) => {
                    let dir_name = tmp.display().to_string();

                    match SystemTime::now().duration_since(SystemTime::UNIX_EPOCH) {
                        Ok(dur) => {
                            string_concat!(dir_name, dur.as_secs().to_string())
                        }
                        _ => dir_name,
                    }
                }
                _ => "/tmp/".to_string()
            }
        };
    };

    match client.get(target_url).send().await {
        Ok(res) if res.status().is_success() => {
            let u = res.url().as_str();

            let rd = if target_url != u {
                Some(u.into())
            } else {
                None
            };

            let mut stream = res.bytes_stream();
            let mut data: BytesMut = BytesMut::new();
            let mut file: Option<tokio::fs::File> = None;
            let mut file_path = String::new();

            while let Some(item) = stream.next().await {
                match item {
                    Ok(text) => {
                        let wrote_disk = file.is_some();

                        // perform operations entire in memory to build resource
                        if !wrote_disk && data.capacity() < 8192 {
                            data.put(text);
                        } else {
                            if !wrote_disk {
                                file_path = string_concat!(
                                    TMP_DIR,
                                    &utf8_percent_encode(target_url, NON_ALPHANUMERIC).to_string()
                                );
                                match tokio::fs::File::create(&file_path).await {
                                    Ok(f) => {
                                        let file = file.insert(f);

                                        data.put(text);

                                        match file.write_all(data.as_bytes()).await {
                                            Ok(_) => {
                                                data.clear();
                                            }
                                            _ => (),
                                        };
                                    }
                                    _ => data.put(text),
                                };
                            } else {
                                match &file.as_mut().unwrap().write_all(&text).await {
                                    Ok(_) => (),
                                    _ => data.put(text),
                                };
                            }
                        }
                    }
                    _ => (),
                }
            }

            // get data from disk
            (
                Some(if file.is_some() {
                    let mut buffer = vec![];

                    match tokio::fs::File::open(&file_path).await {
                        Ok(mut b) => match b.read_to_end(&mut buffer).await {
                            _ => (),
                        },
                        _ => (),
                    };

                    match tokio::fs::remove_file(file_path).await {
                        _ => (),
                    };

                    buffer.into()
                } else {
                    data.into()
                }),
                rd,
            )
        }
        Ok(_) => (None, None),
        Err(_) => {
            log("- error parsing html text {}", &target_url);
            (None, None)
        }
    }
}

#[cfg(feature = "chrome")]
/// Wait for page to fully load
// return is (contents, state)
pub async fn wait_for_page_load(page: &chromiumoxide::Page) -> (CowStr, CowStr) {
    let time_wait = std::time::Duration::from_millis(1);
    let mut counter_retry = 0;
    // to do about 1 second
    let retry_max = 1000;

    // fast navigation needs to wait for the browser to get ready, simply retry this op
    loop {
        match page
            .evaluate(
                r#"() =>
                 new Promise((resolve) => {
                    const doResolve = (x) => requestAnimationFrame(() => {
                        resolve(x)
                    })
                    if (document.readyState === 'complete') {
                          doResolve('pre-loaded')
                    } else {
                        addEventListener('load', () => {
                            doResolve('loaded')
                        })
                    }
                })
                "#,
            )
            .await
        {
            Ok(x) => {
                let contents = page.content().await.unwrap_or_default();

                if !contents.is_empty() {
                    return (
                        contents.into(),
                        x.into_value::<CowStr>()
                            .unwrap_or("failure_into_value".into()),
                    );
                }
            }

            Err(err) => {
                log("- error page.evaluate {}", err.to_string());
            }
        }

        if counter_retry == retry_max {
            return ("".into(), "failure_max_retries_exceeded".into());
        }

        tokio::time::sleep(time_wait).await;
        counter_retry += 1;
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum CachedFetchChromeState {
    Done,
    Fetching,
}

#[derive(Debug, Clone)]
struct CachedFetchChrome {
    pub payload: Option<CowStr>,
    pub state: CachedFetchChromeState,
}

static CACHE_FETCH_CHROME: once_cell::sync::Lazy<moka::future::Cache<CowStr, CachedFetchChrome>> =
    once_cell::sync::Lazy::new(|| {
        moka::future::Cache::builder()
            .max_capacity(1000)
            .time_to_idle(Duration::from_secs(60))
            .build()
    });

fn get_job_max_per_domain() -> (CowStr, usize) {
    let key: CowStr = "JOBS_MAX_PER_DOMAIN".into();

    if let Ok(x) = std::env::var(key.as_str()) {
        if let Ok(x) = x.parse::<usize>() {
            if x > 0 {
                return (key, x);
            }
        }
    }

    (key, 2)
}

static LIMIT_PER_DOMAIN_MAP: once_cell::sync::Lazy<moka::future::Cache<CowStr, Arc<AtomicUsize>>> =
    once_cell::sync::Lazy::new(|| {
        moka::future::Cache::builder()
            .max_capacity(1000)
            .time_to_idle(Duration::from_secs(300))
            .build()
    });

fn get_jobs_max_scraping() -> (CowStr, usize) {
    let key: CowStr = "JOBS_MAX_SCRAPING".into();

    if let Ok(x) = std::env::var(key.as_str()) {
        if let Ok(x) = x.parse::<usize>() {
            if x > 0 {
                return (key, x);
            }
        }
    }

    (key, num_cpus::get())
}

struct TasksLimits {
    // key: URI, value: limit
    map: dashmap::DashMap<CowStr, usize>,
    time_wait: Duration,
}

impl TasksLimits {
    pub fn new() -> Self {
        Self {
            map: dashmap::DashMap::new(),
            time_wait: Duration::from_millis(250),
        }
    }
}

impl TasksLimits {
    pub async fn task_wait_until_allowed(
        &self,
        key: CowStr,
        limit_value: usize,
    ) -> anyhow::Result<()> {
        use dashmap::mapref::entry::Entry;

        loop {
            if let Some(entry) = self.map.try_entry(key.clone()) {
                match entry {
                    Entry::Occupied(mut x) => {
                        let value = x.get_mut();
                        if *value < limit_value {
                            value.checked_add(1).expect("add one");
                            break;
                        }
                    }
                    Entry::Vacant(x) => {
                        x.insert(1);
                        break;
                    }
                };
            }

            tokio::time::sleep(self.time_wait).await;
        }

        Ok(())
    }

    pub async fn task_finish(&self, key: CowStr) -> anyhow::Result<()> {
        use dashmap::mapref::entry::Entry;

        loop {
            if let Some(entry) = self.map.try_entry(key.clone()) {
                match entry {
                    Entry::Occupied(mut x) => {
                        let value = x.get_mut();
                        value.checked_sub(1).expect("subtract 1");
                        break;
                    }
                    _ => {
                        return Err(anyhow::anyhow!(
                            "Cannot finish a task that is not initialized."
                        ));
                    }
                };
            }

            tokio::time::sleep(self.time_wait).await;
        }

        Ok(())
    }
}

// moka does coalesces all calls, meaning they are in a queue, so if there are 100000 waits, all of them will run sync when one resolved, we can't limit anything
// dashmap has try that yields at once
// this will change after https://github.com/moka-rs/moka/issues/227 in which case most likely dashmap is not required
static LIMIT_WORK_STATES: once_cell::sync::Lazy<(TasksLimits, Vec<(CowStr, usize)>)> =
    once_cell::sync::Lazy::new(|| {
        (
            TasksLimits::new(),
            vec![get_jobs_max_scraping(), get_job_max_per_domain()],
        )
    });

static LIMIT_PER_DOMAIN: once_cell::sync::Lazy<usize> = once_cell::sync::Lazy::new(|| {
    if let Ok(x) = std::env::var("JOBS_MAX_PER_DOMAIN") {
        if let Ok(x) = x.parse::<usize>() {
            if x > 0 {
                return x;
            }
        }
    }

    2
});

/// Prevents double visited URIs when it's actually the same URI ending with a slash
pub fn get_uri_sanitized<TInput: Into<String>>(input: TInput) -> CowStr {
    let mut target = input.into();

    if target.ends_with('/') {
        target.pop();
    }

    return target.into();
}

#[cfg(feature = "chrome")]
/// Perform a network request to a resource extracting all content as text streaming via chrome.
pub async fn fetch_page_html_chrome(
    target_uri_input: &str,
    browser_input: std::sync::Arc<tokio::sync::RwLock<chromiumoxide::Browser>>,
) -> Option<bytes::Bytes> {
    let target_uri = get_uri_sanitized(target_uri_input);
    let target_uri_parsed = url::Url::parse(&target_uri.as_str())
        .expect(&format!("failed to parse uri: {}", &target_uri));
    let domain: CowStr = target_uri_parsed
        .domain()
        .expect("failed to get domain.")
        .into();

    let limits = &LIMIT_WORK_STATES.0;
    let limits_jobs = &LIMIT_WORK_STATES.1.get(0).expect("INFALLIBLE");

    {
        let time_wait = std::time::Duration::from_millis(250);

        loop {
            let entry = CACHE_FETCH_CHROME
                .entry_by_ref(&target_uri)
                .or_insert_with(async {
                    CachedFetchChrome {
                        payload: Default::default(),
                        state: CachedFetchChromeState::Fetching,
                    }
                })
                .await;

            if entry.is_fresh() {
                break;
            }

            let CachedFetchChrome { payload, state } = entry.value();

            if state == &CachedFetchChromeState::Done {
                return Some(payload.as_ref().expect("INFALLIBLE").to_string().into());
            }

            tokio::time::sleep(time_wait).await;
        }
    }

    let task_wait_per_domain = async {
        let time_wait = std::time::Duration::from_millis(250);
        // @TODO needs to be revised when adding proxies
        let limit = LIMIT_PER_DOMAIN.clone();

        loop {
            let entry = LIMIT_PER_DOMAIN_MAP
                .entry_by_ref(&domain)
                .or_insert_with(async { Arc::new(AtomicUsize::new(1)) })
                .await;

            if entry.is_fresh() {
                break;
            }

            let value_atom = entry.into_value();
            let value = value_atom.load(Ordering::Relaxed);

            if value < limit {
                value_atom.fetch_add(1, Ordering::Relaxed);
                break;
            } else {
                tokio::time::sleep(time_wait).await;
            }
        }
    };

    let _ = tokio::join!(
        task_wait_per_domain,
        limits.task_wait_until_allowed(limits_jobs.0.clone(), limits_jobs.1)
    );

    let time_wait = std::time::Duration::from_millis(1);
    let mut counter_retry = 0;
    // to do about 1 second
    let retry_max = 1000;
    #[allow(unused_assignments)]
    let retry_next = move || async move {
        if counter_retry == retry_max {
            return Ok::<String, chromiumoxide::error::CdpError>(
                "failure_max_retries_exceeded".to_string(),
            );
        }

        tokio::time::sleep(time_wait).await;
        counter_retry += 1;
        return Ok::<String, chromiumoxide::error::CdpError>("".to_string());
    };

    let close = |page: Option<chromiumoxide::Page>| {
        let browser_input = browser_input.clone();

        async move {
            let task_close_browser = async {
                if let Some(page) = page {
                    let _ = page.close().await;
                }

                if !std::env::var("CHROME_URL").is_ok() {
                    let mut browser = browser_input.write().await;
                    let _ = browser.close().await;
                    let _ = browser.wait().await;
                }
            };

            let task_finisher = async {
                if let Some(x) = LIMIT_PER_DOMAIN_MAP.get(&domain).await {
                    x.fetch_sub(1, Ordering::Relaxed);
                }
                limits
                    .task_finish(limits_jobs.0.clone())
                    .await
                    .expect("INFALLIBLE");
            };

            let _ = tokio::join!(task_close_browser, task_finisher);
        }
    };

    loop {
        match browser_input
            .read()
            .await
            .new_page(target_uri.as_str())
            .await
        {
            Ok(page) => {
                if cfg!(feature = "chrome_stealth") {
                    let _ = page.enable_stealth_mode();
                }
                let (contents, _) = wait_for_page_load(&page).await;

                tokio::join!(
                    CACHE_FETCH_CHROME.insert(
                        target_uri.clone(),
                        CachedFetchChrome {
                            payload: Some(contents.as_str().into()),
                            state: CachedFetchChromeState::Done,
                        },
                    ),
                    close(Some(page))
                );

                return Some(contents.to_string().into());
            }
            result => {
                if let Err(err) = result {
                    match err {
                        chromiumoxide::error::CdpError::ChromeMessage(msg) => {
                            if msg == "net::ERR_ABORTED" {
                                let result_retry = retry_next().await.expect("INFALLIBLE");

                                if result_retry.is_empty() {
                                    continue;
                                }
                            }
                        }
                        _ => {}
                    };
                }

                close(None).await;
                return Some("".into());
            }
        };
    }
}

/// log to console if configuration verbose.
pub fn log(message: &'static str, data: impl AsRef<str>) {
    if log_enabled!(Level::Info) {
        info!("{message} - {}", data.as_ref());
    }
}

#[cfg(feature = "control")]
/// determine action
#[derive(PartialEq, Debug)]
pub enum Handler {
    /// crawl start state
    Start,
    /// crawl pause state
    Pause,
    /// crawl resume
    Resume,
    /// crawl shutdown
    Shutdown,
}

#[cfg(feature = "control")]
lazy_static! {
    /// control handle for crawls
    pub static ref CONTROLLER: std::sync::Arc<tokio::sync::Mutex<(tokio::sync::watch::Sender<(String, Handler)>, tokio::sync::watch::Receiver<(String, Handler)>)>> = std::sync::Arc::new(tokio::sync::Mutex::new(tokio::sync::watch::channel(("handles".to_string(), Handler::Start))));
}

#[cfg(feature = "control")]
/// pause a target website running crawl
pub async fn pause(domain: &str) {
    let s = CONTROLLER.clone();

    match s.lock().await.0.send((domain.into(), Handler::Pause)) {
        _ => (),
    };
}

#[cfg(feature = "control")]
/// resume a target website crawl
pub async fn resume(domain: &str) {
    let s = CONTROLLER.clone();

    match s.lock().await.0.send((domain.into(), Handler::Resume)) {
        _ => (),
    };
}

#[cfg(feature = "control")]
/// shutdown a target website crawl
pub async fn shutdown(domain: &str) {
    let s = CONTROLLER.clone();

    match s.lock().await.0.send((domain.into(), Handler::Shutdown)) {
        _ => (),
    };
}

#[cfg(feature = "control")]
/// reset a target website crawl
pub async fn reset(domain: &str) {
    let s = CONTROLLER.clone();

    match s.lock().await.0.send((domain.into(), Handler::Start)) {
        _ => (),
    };
}
