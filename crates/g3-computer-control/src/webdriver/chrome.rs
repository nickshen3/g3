use super::{WebDriverController, WebElement};
use anyhow::{Context, Result};
use async_trait::async_trait;
use fantoccini::{Client, ClientBuilder};
use serde_json::Value;
use std::time::Duration;

/// ChromeDriver WebDriver controller with headless support
pub struct ChromeDriver {
    client: Client,
}

/// Stealth script to hide automation indicators from bot detection
const STEALTH_SCRIPT: &str = r#"
    (function() {
        'use strict';
        
        // 1. Override navigator.webdriver to return undefined (like a real browser)
        Object.defineProperty(navigator, 'webdriver', {
            get: () => undefined,
            configurable: true
        });
        
        // 2. Add realistic chrome object that real Chrome has
        if (!window.chrome) {
            window.chrome = {};
        }
        window.chrome.runtime = {
            connect: function() {},
            sendMessage: function() {},
            onMessage: { addListener: function() {} },
            onConnect: { addListener: function() {} },
            id: undefined
        };
        window.chrome.loadTimes = function() {
            return {
                commitLoadTime: Date.now() / 1000,
                connectionInfo: 'h2',
                finishDocumentLoadTime: Date.now() / 1000,
                finishLoadTime: Date.now() / 1000,
                firstPaintAfterLoadTime: 0,
                firstPaintTime: Date.now() / 1000,
                navigationType: 'Other',
                npnNegotiatedProtocol: 'h2',
                requestTime: Date.now() / 1000,
                startLoadTime: Date.now() / 1000,
                wasAlternateProtocolAvailable: false,
                wasFetchedViaSpdy: true,
                wasNpnNegotiated: true
            };
        };
        window.chrome.csi = function() {
            return {
                onloadT: Date.now(),
                pageT: Date.now() - performance.timing.navigationStart,
                startE: performance.timing.navigationStart,
                tran: 15
            };
        };
        
        // 3. Add realistic plugins array (headless Chrome has empty plugins)
        Object.defineProperty(navigator, 'plugins', {
            get: () => {
                const plugins = [
                    { name: 'Chrome PDF Plugin', filename: 'internal-pdf-viewer', description: 'Portable Document Format' },
                    { name: 'Chrome PDF Viewer', filename: 'mhjfbmdgcfjbbpaeojofohoefgiehjai', description: '' },
                    { name: 'Native Client', filename: 'internal-nacl-plugin', description: '' }
                ];
                plugins.item = (i) => plugins[i] || null;
                plugins.namedItem = (name) => plugins.find(p => p.name === name) || null;
                plugins.refresh = () => {};
                Object.setPrototypeOf(plugins, PluginArray.prototype);
                return plugins;
            },
            configurable: true
        });
        
        // 4. Add realistic mimeTypes
        Object.defineProperty(navigator, 'mimeTypes', {
            get: () => {
                const mimeTypes = [
                    { type: 'application/pdf', suffixes: 'pdf', description: 'Portable Document Format' },
                    { type: 'application/x-google-chrome-pdf', suffixes: 'pdf', description: 'Portable Document Format' }
                ];
                mimeTypes.item = (i) => mimeTypes[i] || null;
                mimeTypes.namedItem = (name) => mimeTypes.find(m => m.type === name) || null;
                Object.setPrototypeOf(mimeTypes, MimeTypeArray.prototype);
                return mimeTypes;
            },
            configurable: true
        });
        
        // 5. Fix permissions API to not reveal automation
        const originalQuery = window.navigator.permissions?.query;
        if (originalQuery) {
            window.navigator.permissions.query = (parameters) => {
                if (parameters.name === 'notifications') {
                    return Promise.resolve({ state: Notification.permission, onchange: null });
                }
                return originalQuery.call(window.navigator.permissions, parameters);
            };
        }
        
        // 6. Override languages to have realistic values
        Object.defineProperty(navigator, 'languages', {
            get: () => ['en-US', 'en'],
            configurable: true
        });
        
        // 7. Fix hardwareConcurrency (headless often shows different values)
        Object.defineProperty(navigator, 'hardwareConcurrency', {
            get: () => 8,
            configurable: true
        });
        
        // 8. Fix deviceMemory
        Object.defineProperty(navigator, 'deviceMemory', {
            get: () => 8,
            configurable: true
        });
        
        // 9. Remove automation-related properties from window
        delete window.cdc_adoQpoasnfa76pfcZLmcfl_Array;
        delete window.cdc_adoQpoasnfa76pfcZLmcfl_Promise;
        delete window.cdc_adoQpoasnfa76pfcZLmcfl_Symbol;
        
        // 10. Fix toString methods to not reveal native code modifications
        const originalToString = Function.prototype.toString;
        Function.prototype.toString = function() {
            if (this === navigator.permissions.query) {
                return 'function query() { [native code] }';
            }
            return originalToString.call(this);
        };
    })();
"#;

impl ChromeDriver {
    /// Create a new ChromeDriver instance in headless mode
    ///
    /// This will connect to ChromeDriver running on the default port (9515).
    /// ChromeDriver must be installed and available in PATH.
    pub async fn new_headless() -> Result<Self> {
        Self::with_port_headless(9515).await
    }

    /// Create a new ChromeDriver instance with Chrome for Testing binary
    pub async fn new_headless_with_binary(chrome_binary: &str) -> Result<Self> {
        Self::with_port_headless_and_binary(9515, Some(chrome_binary)).await
    }

    /// Create a new ChromeDriver instance with a custom port in headless mode
    pub async fn with_port_headless(port: u16) -> Result<Self> {
        Self::with_port_headless_and_binary(port, None).await
    }

    /// Create a new ChromeDriver instance with a custom port and optional Chrome binary path
    pub async fn with_port_headless_and_binary(port: u16, chrome_binary: Option<&str>) -> Result<Self> {
        let url = format!("http://localhost:{}", port);

        let mut caps = serde_json::Map::new();
        caps.insert(
            "browserName".to_string(),
            Value::String("chrome".to_string()),
        );

        // Set up Chrome options for headless mode
        let mut chrome_options = serde_json::Map::new();
        chrome_options.insert(
            "args".to_string(),
            Value::Array(vec![
                // Use a unique temp directory to avoid conflicts with running Chrome instances
                Value::String(format!("--user-data-dir=/tmp/g3-chrome-{}", std::process::id())),
                Value::String("--headless=new".to_string()),
                Value::String("--disable-gpu".to_string()),
                Value::String("--no-sandbox".to_string()),
                Value::String("--disable-dev-shm-usage".to_string()),
                Value::String("--window-size=1920,1080".to_string()),
                Value::String("--disable-blink-features=AutomationControlled".to_string()),
                // Stealth: Set a realistic user-agent (removes HeadlessChrome identifier)
                Value::String("--user-agent=Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36".to_string()),
                // Stealth: Disable automation-related info bars
                Value::String("--disable-infobars".to_string()),
                // Stealth: Set realistic language
                Value::String("--lang=en-US,en".to_string()),
                // Stealth: Disable extensions to avoid detection
                Value::String("--disable-extensions".to_string()),
            ]),
        );

        // Exclude automation switches to hide webdriver detection
        chrome_options.insert(
            "excludeSwitches".to_string(),
            Value::Array(vec![
                Value::String("enable-automation".to_string()),
            ]),
        );

        // Disable automation extension
        chrome_options.insert(
            "useAutomationExtension".to_string(),
            Value::Bool(false),
        );

        // If a custom Chrome binary is specified, use it
        if let Some(binary) = chrome_binary {
            chrome_options.insert("binary".to_string(), Value::String(binary.to_string()));
        }

        caps.insert(
            "goog:chromeOptions".to_string(),
            Value::Object(chrome_options),
        );

        // Use a timeout for the connection attempt to avoid hanging indefinitely
        let mut builder = ClientBuilder::native();
        let connect_future = builder
            .capabilities(caps)
            .connect(&url);
        
        let client = tokio::time::timeout(Duration::from_secs(30), connect_future)
            .await
            .context("Connection to ChromeDriver timed out after 30 seconds")?
            .context("Failed to connect to ChromeDriver")?;

        let driver = Self { client };
        
        // Inject stealth script immediately after connection
        // This ensures it runs before any navigation and on every new document
        // Ignore errors as this is best-effort stealth
        let _ = driver.client.execute(STEALTH_SCRIPT, vec![]).await;
        
        Ok(driver)
    }

    /// Go back in browser history
    pub async fn back(&mut self) -> Result<()> {
        self.client.back().await?;
        Ok(())
    }

    /// Go forward in browser history
    pub async fn forward(&mut self) -> Result<()> {
        self.client.forward().await?;
        Ok(())
    }

    /// Refresh the current page
    pub async fn refresh(&mut self) -> Result<()> {
        self.client.refresh().await?;
        Ok(())
    }

    /// Get all window handles
    pub async fn window_handles(&mut self) -> Result<Vec<String>> {
        let handles = self.client.windows().await?;
        Ok(handles.into_iter().map(|h| h.into()).collect())
    }

    /// Switch to a window by handle
    pub async fn switch_to_window(&mut self, handle: &str) -> Result<()> {
        let window_handle: fantoccini::wd::WindowHandle = handle.to_string().try_into()?;
        self.client.switch_to_window(window_handle).await?;
        Ok(())
    }

    /// Get the current window handle
    pub async fn current_window_handle(&mut self) -> Result<String> {
        Ok(self.client.window().await?.into())
    }

    /// Close the current window
    pub async fn close_window(&mut self) -> Result<()> {
        self.client.close_window().await?;
        Ok(())
    }

    /// Create a new window/tab
    pub async fn new_window(&mut self, is_tab: bool) -> Result<String> {
        let response = self.client.new_window(is_tab).await?;
        Ok(response.handle.into())
    }

    /// Get cookies
    pub async fn get_cookies(&mut self) -> Result<Vec<fantoccini::cookies::Cookie<'static>>> {
        Ok(self.client.get_all_cookies().await?)
    }

    /// Add a cookie
    pub async fn add_cookie(&mut self, cookie: fantoccini::cookies::Cookie<'static>) -> Result<()> {
        self.client.add_cookie(cookie).await?;
        Ok(())
    }

    /// Delete all cookies
    pub async fn delete_all_cookies(&mut self) -> Result<()> {
        self.client.delete_all_cookies().await?;
        Ok(())
    }

    /// Wait for an element to appear (with timeout)
    pub async fn wait_for_element(
        &mut self,
        selector: &str,
        timeout: Duration,
    ) -> Result<WebElement> {
        let start = std::time::Instant::now();
        let poll_interval = Duration::from_millis(100);

        loop {
            if let Ok(elem) = self.find_element(selector).await {
                return Ok(elem);
            }

            if start.elapsed() >= timeout {
                anyhow::bail!("Timeout waiting for element: {}", selector);
            }

            tokio::time::sleep(poll_interval).await;
        }
    }

    /// Wait for an element to be visible (with timeout)
    pub async fn wait_for_visible(
        &mut self,
        selector: &str,
        timeout: Duration,
    ) -> Result<WebElement> {
        let start = std::time::Instant::now();
        let poll_interval = Duration::from_millis(100);

        loop {
            if let Ok(elem) = self.find_element(selector).await {
                if elem.is_displayed().await.unwrap_or(false) {
                    return Ok(elem);
                }
            }

            if start.elapsed() >= timeout {
                anyhow::bail!("Timeout waiting for element to be visible: {}", selector);
            }

            tokio::time::sleep(poll_interval).await;
        }
    }
}

#[async_trait]
impl WebDriverController for ChromeDriver {
    async fn navigate(&mut self, url: &str) -> Result<()> {
        self.client.goto(url).await?;
        // Inject stealth script after navigation to hide automation indicators
        // Ignore errors as some pages may have strict CSP
        let _ = self.client.execute(STEALTH_SCRIPT, vec![]).await;
        Ok(())
    }

    async fn current_url(&self) -> Result<String> {
        Ok(self.client.current_url().await?.to_string())
    }

    async fn title(&self) -> Result<String> {
        Ok(self.client.title().await?)
    }

    async fn find_element(&mut self, selector: &str) -> Result<WebElement> {
        let elem = self
            .client
            .find(fantoccini::Locator::Css(selector))
            .await
            .context(format!(
                "Failed to find element with selector: {}",
                selector
            ))?;
        Ok(WebElement { inner: elem })
    }

    async fn find_elements(&mut self, selector: &str) -> Result<Vec<WebElement>> {
        let elems = self
            .client
            .find_all(fantoccini::Locator::Css(selector))
            .await?;
        Ok(elems
            .into_iter()
            .map(|inner| WebElement { inner })
            .collect())
    }

    async fn execute_script(&mut self, script: &str, args: Vec<Value>) -> Result<Value> {
        Ok(self.client.execute(script, args).await?)
    }

    async fn page_source(&self) -> Result<String> {
        Ok(self.client.source().await?)
    }

    async fn screenshot(&mut self, path: &str) -> Result<()> {
        let screenshot_data = self.client.screenshot().await?;

        // Expand tilde in path
        let expanded_path = shellexpand::tilde(path);
        let path_str = expanded_path.as_ref();

        // Create parent directories if needed
        if let Some(parent) = std::path::Path::new(path_str).parent() {
            std::fs::create_dir_all(parent)
                .context("Failed to create parent directories for screenshot")?;
        }

        std::fs::write(path_str, screenshot_data).context("Failed to write screenshot to file")?;

        Ok(())
    }

    async fn close(&mut self) -> Result<()> {
        self.client.close_window().await?;
        Ok(())
    }

    async fn quit(mut self) -> Result<()> {
        self.client.close().await?;
        Ok(())
    }
}
