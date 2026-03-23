use crate::config::{Config, ConfigError};
use crate::db;
use std::path::Path;

pub(crate) fn load_config_or_exit(config_path: &str) -> Config {
    match Config::load(Path::new(config_path)) {
        Ok(c) => c,
        Err(ConfigError::NotFound(_)) => {
            eprintln!();
            eprintln!("  error: config not found");
            eprintln!("    hint: run `paygate init` to create paygate.toml");
            std::process::exit(1);
        }
        Err(e) => {
            eprintln!();
            eprintln!("  error: {e}");
            std::process::exit(1);
        }
    }
}

pub(crate) fn open_db_reader() -> Option<db::DbReader> {
    let path = "paygate.db";
    if Path::new(path).exists() {
        Some(db::DbReader::new(path))
    } else {
        None
    }
}

pub(crate) fn truncate_address(addr: &str) -> String {
    if addr.len() >= 12 {
        format!("{}...{}", &addr[..6], &addr[addr.len() - 4..])
    } else {
        addr.to_string()
    }
}

pub(crate) fn truncate_id(id: &str) -> String {
    if id.len() > 12 {
        format!("{}..", &id[..10])
    } else {
        id.to_string()
    }
}

pub(crate) fn format_number(n: u64) -> String {
    if n == 0 {
        return "0".to_string();
    }
    let s = n.to_string();
    let mut result = String::new();
    for (i, c) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            result.push(',');
        }
        result.push(c);
    }
    result.chars().rev().collect()
}

pub(crate) fn format_file_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

/// Escape a string for safe HTML interpolation, preventing XSS.
pub(crate) fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#x27;")
}

pub(crate) fn prompt(label: &str, default: &str) -> String {
    use std::io::{BufRead, Write};

    if default.is_empty() {
        eprint!("{label}: ");
    } else {
        eprint!("{label} [{default}]: ");
    }
    std::io::stderr().flush().ok();

    let mut input = String::new();
    std::io::stdin().lock().read_line(&mut input).unwrap_or(0);
    let trimmed = input.trim();

    if trimmed.is_empty() {
        default.to_string()
    } else {
        trimmed.to_string()
    }
}

pub(crate) fn print_pricing_html(config: &Config) {
    use crate::config::parse_price_to_base_units;
    use paygate_common::types::{format_usd, TOKEN_DECIMALS};

    let mut rows = String::new();

    let mut endpoints: Vec<_> = config.pricing.endpoints.iter().collect();
    endpoints.sort_by_key(|(k, _)| k.clone());

    for (endpoint, price_str) in &endpoints {
        let base = parse_price_to_base_units(price_str).unwrap_or(0);
        let price_display = if base == 0 {
            "free".to_string()
        } else {
            format_usd(base, TOKEN_DECIMALS)
        };
        rows.push_str(&format!(
            "        <tr><td><code>{}</code></td><td>{}</td></tr>\n",
            html_escape(endpoint),
            html_escape(&price_display),
        ));
    }

    let default_base = parse_price_to_base_units(&config.pricing.default_price).unwrap_or(1000);
    rows.push_str(&format!(
        "        <tr><td><code>* (default)</code></td><td>{}</td></tr>\n",
        html_escape(&format_usd(default_base, TOKEN_DECIMALS))
    ));

    let provider_name_raw = if config.provider.name.is_empty() {
        "PayGate API"
    } else {
        &config.provider.name
    };
    let provider_name = html_escape(provider_name_raw);

    println!(
        r#"<!DOCTYPE html>
<html>
<head>
  <meta charset="utf-8">
  <title>{name} — Pricing</title>
  <style>
    body {{ font-family: system-ui, sans-serif; max-width: 640px; margin: 4rem auto; padding: 0 1rem; }}
    h1 {{ font-size: 1.5rem; }}
    table {{ border-collapse: collapse; width: 100%; }}
    th, td {{ text-align: left; padding: 0.5rem 1rem; border-bottom: 1px solid #eee; }}
    th {{ font-weight: 600; }}
    code {{ background: #f5f5f5; padding: 0.1em 0.3em; border-radius: 3px; }}
    .note {{ color: #666; font-size: 0.9rem; margin-top: 2rem; }}
  </style>
</head>
<body>
  <h1>{name} — Pricing</h1>
  <p>Pay per request using USDC on Tempo.</p>
  <table>
    <thead>
      <tr><th>Endpoint</th><th>Price</th></tr>
    </thead>
    <tbody>
{rows}    </tbody>
  </table>
  <p class="note">Payment: send USDC to <code>{address}</code> on Tempo, then retry with <code>X-Payment-Tx</code> header.</p>
</body>
</html>"#,
        name = provider_name,
        rows = rows,
        address = html_escape(&truncate_address(&config.provider.address)),
    );
}

pub(crate) fn print_revenue_empty() {
    eprintln!();
    eprintln!("  Revenue Summary");
    eprintln!("  \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}");
    eprintln!("  No payments recorded yet.");
    eprintln!();
    eprintln!("  hint: run `paygate test` to verify your setup, or send a request to your gateway");
}
