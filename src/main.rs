use chrono::{DateTime, Datelike, TimeZone, Timelike, Utc};
use chrono_tz::Tz;
use rand::Rng;
use reqwest::Client;
use sha2::{Digest, Sha256};
use std::time::Duration;
use tracing::{error, info, warn};

#[tokio::main]
async fn main() {
    // Initialize tracing first - this ensures all errors are visible
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .with_timer(tracing_subscriber::fmt::time::ChronoUtc::rfc_3339())
        .init();

    if let Err(e) = run().await {
        eprintln!("Fatal error: {}", e);
        error!("Fatal error: {}", e);
        std::process::exit(1);
    }
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    // Get configuration from environment variables
    let router_ip = std::env::var("ROUTER_IP")
        .map_err(|_| "ROUTER_IP environment variable is required")?;
    let username = std::env::var("ROUTER_USERNAME")
        .map_err(|_| "ROUTER_USERNAME environment variable is required")?;
    let password = std::env::var("ROUTER_PASSWORD")
        .map_err(|_| "ROUTER_PASSWORD environment variable is required")?;
    let device_mac = std::env::var("DEVICE_MAC")
        .map_err(|_| "DEVICE_MAC environment variable is required")?;
    let timezone_str = std::env::var("TIMEZONE").unwrap_or_else(|_| "UTC".to_string());
    let cron_schedule = std::env::var("CRON_SCHEDULE").unwrap_or_else(|_| "0 3 * * *".to_string());

    let timezone: Tz = timezone_str
        .parse()
        .map_err(|_| format!("Invalid timezone: {}", timezone_str))?;

    info!("Router Reboot Scheduler starting...");
    info!("Router IP: {}", router_ip);
    info!("Device MAC: {}", device_mac);
    info!("Username: {}", username);
    info!("Timezone: {}", timezone_str);
    info!("Cron schedule: {}", cron_schedule);

    let scheduler = RouterRebootScheduler::new(router_ip, username, password, device_mac)?;

    // Optionally perform an immediate reboot for testing
    if std::env::var("IMMEDIATE_REBOOT")
        .unwrap_or_else(|_| "false".to_string())
        .to_lowercase()
        == "true"
    {
        info!("Performing immediate reboot (IMMEDIATE_REBOOT=true)...");
        scheduler.perform_reboot().await?;
    }

    // Start the scheduler
    scheduler.run_scheduler(&cron_schedule, timezone).await?;

    Ok(())
}

struct RouterRebootScheduler {
    router_url: String,
    username: String,
    password: String,
    device_mac: String,
    client: Client,
}

impl RouterRebootScheduler {
    fn new(
        router_ip: String,
        username: String,
        password: String,
        device_mac: String,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let router_url = format!("http://{}", router_ip);
        let client = Client::builder()
            .cookie_store(true)
            .timeout(Duration::from_secs(10))
            .user_agent("Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
            .build()?;

        Ok(Self {
            router_url,
            username,
            password,
            device_mac,
            client,
        })
    }

    fn random_string(&self, length: usize) -> String {
        const CHARS: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ";
        let mut rng = rand::thread_rng();
        (0..length)
            .map(|_| {
                let idx = rng.gen_range(0..CHARS.len());
                CHARS[idx] as char
            })
            .collect()
    }

    fn sha256(&self, text: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(text.as_bytes());
        format!("{:x}", hasher.finalize())
    }

    async fn get_nonce(&self, id: &str) -> Result<String, Box<dyn std::error::Error>> {
        let url = format!("{}/get_Nonce.cgi", self.router_url);
        let payload = serde_json::json!({ "id": id });

        let response = self.client.post(&url).json(&payload).send().await?;
        let status = response.status();
        if !status.is_success() {
            return Err(format!("Failed to get nonce: HTTP {}", status).into());
        }

        let data: serde_json::Value = response.json().await?;
        data.get("nonce")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| "No nonce in response".into())
    }

    async fn login(&self) -> Result<(), Box<dyn std::error::Error>> {
        // Generate id once and use it for both nonce request and login request
        let login_id = self.random_string(10);
        let nonce = self.get_nonce(&login_id).await?;
        info!("Got nonce, authenticating...");

        let cnonce = self.random_string(32);

        // Calculate login authorization hash: sha256(username:nonce:password:cnonce)
        let auth_string = format!("{}:{}:{}:{}", self.username, nonce, self.password, cnonce);
        let login_authorization = self.sha256(&auth_string);

        let url = format!("{}/login_v2.cgi", self.router_url);
        let mut params = std::collections::HashMap::new();
        params.insert("group_id", "");
        params.insert("action_mode", "");
        params.insert("action_script", "");
        params.insert("action_wait", "5");
        params.insert("current_page", "Main_Login.asp");
        params.insert("next_page", "index.asp");
        params.insert("login_authorization", &login_authorization);
        params.insert("id", &login_id);
        params.insert("cnonce", &cnonce);
        params.insert("login_captcha", "");

        let response = self.client.post(&url).form(&params).send().await?;
        let status = response.status();

        // Check for asus_token cookie first (before consuming response)
        let mut token_found = false;
        for cookie in response.cookies() {
            if cookie.name() == "asus_token" {
                token_found = true;
                info!("Successfully authenticated - token received");
                break;
            }
        }

        // Get response text to check for redirects
        let response_text = response.text().await?;

        if !status.is_success() {
            error!("Login failed: HTTP {}", status);
            error!("Response text: {}", &response_text[..response_text.len().min(200)]);
            return Err(format!("Login failed: HTTP {}", status).into());
        }

        // Check if we got redirected back to login (authentication failed)
        if response_text.contains("Main_Login.asp") || response_text.contains("parent.location.href") {
            error!("Authentication failed - redirected to login page");
            error!("Response text: {}", &response_text[..response_text.len().min(200)]);
            return Err("Authentication failed - invalid username or password. Check your credentials.".into());
        }

        if !token_found {
            error!("Response status: {}", status);
            error!("Response text: {}", &response_text[..response_text.len().min(200)]);
            return Err("Authentication failed - no token received. Check username and password.".into());
        }

        Ok(())
    }

    async fn reboot_device(&self) -> Result<(), Box<dyn std::error::Error>> {
        let url = format!("{}/applyapp.cgi", self.router_url);

        // Create JSON payload and URL encode it
        let json_payload = format!(
            r#"{{"device_list":"{}","action_mode":"device_reboot"}}"#,
            self.device_mac
        );
        let encoded_payload = urlencoding::encode(&json_payload);

        let response = self
            .client
            .post(&url)
            .header("Content-Type", "application/x-www-form-urlencoded; charset=UTF-8")
            .header("X-Requested-With", "XMLHttpRequest")
            .body(encoded_payload.to_string())
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            return Err(format!("Failed to reboot device: HTTP {}", status).into());
        }

        info!("Reboot command sent successfully for device {}", self.device_mac);
        Ok(())
    }

    async fn logout(&self) {
        let url = format!("{}/Logout.asp", self.router_url);
        if let Err(e) = self.client.get(&url).send().await {
            warn!("Logout failed: {}", e);
        } else {
            info!("Logged out");
        }
    }

    async fn perform_reboot(&self) -> Result<(), Box<dyn std::error::Error>> {
        info!("Starting reboot process...");
        self.login().await?;
        self.reboot_device().await?;
        self.logout().await;
        info!("Reboot process completed successfully");
        Ok(())
    }

    fn matches_cron_field(value: i32, field: &str, _min: i32, _max: i32) -> bool {
        if field == "*" {
            return true;
        }
        if let Ok(num) = field.parse::<i32>() {
            return num == value;
        }
        // Handle ranges like "1-5" or steps like "*/5" (simplified - just check exact match for now)
        // min and max are kept for future range/step support
        false
    }

    fn calculate_next_run(cron_parts: &[&str], now: DateTime<Tz>) -> DateTime<Tz> {
        let minute_part = cron_parts[0];
        let hour_part = cron_parts[1];
        let _day_part = cron_parts[2];
        let _month_part = cron_parts[3];
        let _dow_part = cron_parts[4];

        // Parse target minute and hour (for simple cases like "0 3 * * *")
        let target_minute = if minute_part == "*" {
            None
        } else {
            minute_part.parse::<u32>().ok()
        };

        let target_hour = if hour_part == "*" {
            None
        } else {
            hour_part.parse::<u32>().ok()
        };

        // For simple daily schedules (minute and hour specified, rest are *)
        if let (Some(min), Some(hr)) = (target_minute, target_hour) {
            // Get today's date
            let today = now.date_naive();

            // Calculate target time for today
            let target_today = today.and_hms_opt(hr, min, 0).unwrap();
            let mut next = now.timezone().from_local_datetime(&target_today).unwrap();

            // If target time has passed today, go to tomorrow
            if next <= now {
                let tomorrow = today + chrono::Duration::days(1);
                let target_tomorrow = tomorrow.and_hms_opt(hr, min, 0).unwrap();
                next = now.timezone().from_local_datetime(&target_tomorrow).unwrap();
            }

            return next;
        }

        // For more complex schedules, use iterative approach
        // Start from the next minute to avoid matching the current time
        let mut next = now + chrono::Duration::minutes(1);
        let mut attempts = 0;
        const MAX_ATTEMPTS: i32 = 366 * 24 * 60; // Max minutes to look ahead (1 year)

        loop {
            attempts += 1;
            if attempts > MAX_ATTEMPTS {
                // Fallback: add 24 hours if we can't find a match
                next = now + chrono::Duration::hours(24);
                warn!("Could not find next scheduled time, using fallback: {}", next.format("%Y-%m-%d %H:%M:%S %Z"));
                break;
            }

            let minute = next.minute() as i32;
            let hour = next.hour() as i32;
            let day = next.day() as i32;
            let month = next.month() as i32;
            let weekday = next.weekday().num_days_from_monday() as i32; // 0=Monday, 6=Sunday

            // Check if current time matches
            let minute_match = Self::matches_cron_field(minute, minute_part, 0, 59);
            let hour_match = Self::matches_cron_field(hour, hour_part, 0, 23);
            let day_match = Self::matches_cron_field(day, _day_part, 1, 31);
            let month_match = Self::matches_cron_field(month, _month_part, 1, 12);
            let dow_match = Self::matches_cron_field(weekday, _dow_part, 0, 6);

            if minute_match && hour_match && day_match && month_match && dow_match {
                // Found a match, make sure it's in the future
                if next > now {
                    break;
                }
            }

            // Advance to next minute
            next = next + chrono::Duration::minutes(1);
        }

        next
    }

    async fn run_scheduler(
        &self,
        cron_expression: &str,
        timezone: Tz,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Validate cron expression format
        let normalized = cron_expression.trim();
        let parts: Vec<&str> = normalized.split_whitespace().collect();

        if parts.len() != 5 {
            return Err(format!(
                "Invalid cron expression '{}': Expected exactly 5 fields (minute hour day month day-of-week), got {} fields. Example: '0 3 * * *'",
                cron_expression, parts.len()
            ).into());
        }

        let current_time = Utc::now().with_timezone(&timezone);
        info!(
            "Scheduler started. Current time: {}",
            current_time.format("%Y-%m-%d %H:%M:%S %Z")
        );
        info!("Timezone: {}", timezone);
        info!("Cron schedule: {}", cron_expression);
        info!("Router will reboot according to the configured schedule");

        loop {
            let now = Utc::now().with_timezone(&timezone);
            let next_run = Self::calculate_next_run(&parts, now);

            let duration = (next_run - now).to_std()?;
            info!(
                "Next reboot scheduled for: {}",
                next_run.format("%Y-%m-%d %H:%M:%S %Z")
            );

            // Sleep until the scheduled time
            tokio::time::sleep(duration).await;

            // Perform the reboot
            if let Err(e) = self.perform_reboot().await {
                error!("Reboot process failed: {}", e);
            }
        }
    }
}
