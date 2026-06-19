//! CLI administration tool for url-shortener.
//!
//! Provides commands for managing API tokens, viewing statistics,
//! and performing database operations without requiring HTTP API access.
//!
//! # Usage
//!
//! ```bash
//! # Create a new API token
//! cargo run --bin admin -- token create
//!
//! # List all tokens
//! cargo run --bin admin -- token list
//!
//! # Revoke a token
//! cargo run --bin admin -- token revoke "Production API"
//!
//! # View statistics
//! cargo run --bin admin -- stats
//!
//! # Check database connection
//! cargo run --bin admin -- db check
//! ```
//!
//! # Environment Variables
//!
//! - `DATABASE_URL` (required): PostgreSQL connection string
//!
//! # Features
//!
//! - **Token Management**: Create, list, and revoke API tokens
//! - **Statistics**: View link and click counts
//! - **Database Tools**: Connection checks and info queries
//! - **Interactive Prompts**: User-friendly CLI with confirmation dialogs
//! - **Colored Output**: Terminal-friendly formatting using `colored` crate

use url_shortener::domain::repositories::TokenRepository;
use url_shortener::infrastructure::persistence::PgTokenRepository;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use colored::*;
use dialoguer::{Confirm, Input};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use sqlx::PgPool;
use std::sync::Arc;

type HmacSha256 = Hmac<Sha256>;

/// CLI tool for managing url-shortener.
#[derive(Parser)]
#[command(name = "admin")]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

/// Top-level command groups.
#[derive(Subcommand)]
enum Commands {
    /// Manage API tokens
    Token {
        #[command(subcommand)]
        action: TokenAction,
    },

    /// Show statistics
    Stats,

    /// Database operations
    Db {
        #[command(subcommand)]
        action: DbAction,
    },
}

/// Token management subcommands.
#[derive(Subcommand)]
enum TokenAction {
    /// Create a new API token
    Create {
        /// Token name (e.g., "Production API", "Mobile App")
        #[arg(short, long)]
        name: Option<String>,

        /// Custom token value (optional, auto-generated if not provided)
        #[arg(short, long)]
        token: Option<String>,

        /// Skip confirmation prompt
        #[arg(short = 'y', long)]
        yes: bool,
    },

    /// List all tokens
    List,

    /// Revoke a token
    Revoke {
        /// Token name or ID to revoke
        name_or_hash: String,
    },
}

/// Database operation subcommands.
#[derive(Subcommand)]
enum DbAction {
    /// Check database connection
    Check,

    /// Show database info
    Info,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Load environment variables from .env file
    dotenvy::dotenv().ok();

    let cli = Cli::parse();

    // Connect to database
    let database_url = std::env::var("DATABASE_URL").context("DATABASE_URL must be set")?;
    let signing_secret =
        std::env::var("TOKEN_SIGNING_SECRET").context("TOKEN_SIGNING_SECRET must be set")?;

    let pool = PgPool::connect(&database_url)
        .await
        .context("Failed to connect to database")?;

    match cli.command {
        Commands::Token { action } => handle_token_action(action, &pool, &signing_secret).await?,
        Commands::Stats => handle_stats(&pool).await?,
        Commands::Db { action } => handle_db_action(action, &pool).await?,
    }

    Ok(())
}

/// Dispatches token management commands.
async fn handle_token_action(action: TokenAction, pool: &PgPool, secret: &str) -> Result<()> {
    let repo = Arc::new(PgTokenRepository::new(Arc::new(pool.clone())));

    match action {
        TokenAction::Create { name, token, yes } => {
            create_token(repo, name, token, yes, secret).await?;
        }
        TokenAction::List => {
            list_tokens(repo).await?;
        }
        TokenAction::Revoke { name_or_hash } => {
            revoke_token(repo, name_or_hash).await?;
        }
    }

    Ok(())
}

/// Creates a new API token with interactive prompts.
///
/// # Flow
///
/// 1. Prompt for token name (or use provided)
/// 2. Generate random token or use provided value
/// 3. Display token details with warning
/// 4. Confirm creation (unless `--yes` flag)
/// 5. Hash token with SHA-256
/// 6. Store in database
/// 7. Display usage instructions
///
/// # Security
///
/// - Only the SHA-256 hash is stored in the database
/// - Raw token is displayed once and cannot be retrieved later
/// - Tokens are 48 characters (alphanumeric) for high entropy
async fn create_token(
    repo: Arc<PgTokenRepository>,
    name: Option<String>,
    token: Option<String>,
    skip_confirm: bool,
    secret: &str,
) -> Result<()> {
    println!("{}", "🔑 Create API Token".bright_blue().bold());
    println!();

    // Get token name
    let token_name = match name {
        Some(n) => n,
        None => Input::new()
            .with_prompt("Token name")
            .with_initial_text("Production API")
            .interact_text()?,
    };

    // Generate or use provided token
    let token_value = match token {
        Some(t) => {
            println!("{}", "⚠️  Using provided token value".yellow());
            t
        }
        None => {
            let generated = generate_token();
            println!("{}", "✨ Generated new token".green());
            generated
        }
    };

    // Show token details
    println!();
    println!("{}", "Token details:".bright_white().bold());
    println!("  Name:  {}", token_name.cyan());
    println!("  Token: {}", token_value.bright_yellow().bold());
    println!();
    println!(
        "{}",
        "⚠️  IMPORTANT: Save this token now! You won't be able to see it again."
            .red()
            .bold()
    );
    println!();

    // Confirm
    if !skip_confirm {
        let confirmed = Confirm::new()
            .with_prompt("Create this token?")
            .default(true)
            .interact()?;

        if !confirmed {
            println!("{}", "❌ Cancelled".red());
            return Ok(());
        }
    }

    // Hash token
    let token_hash = hash_token(&token_value, secret);

    // Save to database
    repo.create_token(&token_name, &token_hash)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to create token: {}", e))?;

    println!();
    println!("{}", "✅ Token created successfully!".green().bold());
    println!();
    println!("{}", "Add this to your requests:".bright_white());
    println!(
        "  {}: Bearer {}",
        "Authorization".bright_cyan(),
        token_value.bright_yellow()
    );
    println!();
    println!("{}", "Example:".bright_white());
    println!(
        "  curl -H \"Authorization: Bearer {}\" http://localhost:3000/api/stats",
        token_value.bright_yellow()
    );
    println!();

    Ok(())
}

/// Lists all API tokens with status indicators.
///
/// # Output Format
///
/// ```text
/// 📋 API Tokens
///
///   ID  Name                           Created              Status
///   ─────────────────────────────────────────────────────────────────────────
///   1   Production API                 2024-01-15 10:30     ACTIVE
///   2   Mobile App                     2024-01-16 14:20     REVOKED
/// ```
async fn list_tokens(repo: Arc<PgTokenRepository>) -> Result<()> {
    println!("{}", "📋 API Tokens".bright_blue().bold());
    println!();

    let tokens = repo
        .list_tokens()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to list tokens: {}", e))?;

    if tokens.is_empty() {
        println!("{}", "  No tokens found".yellow());
        println!();
        println!(
            "  Create one with: {} admin token create",
            "cargo run --bin".bright_cyan()
        );
        return Ok(());
    }

    println!(
        "  {:<3} {:<30} {:<20} {:<10}",
        "ID".bright_white().bold(),
        "Name".bright_white().bold(),
        "Created".bright_white().bold(),
        "Status".bright_white().bold()
    );
    println!("  {}", "─".repeat(75).bright_black());

    for token in &tokens {
        let status = if token.revoked_at.is_some() {
            "REVOKED".red()
        } else {
            "ACTIVE".green()
        };

        println!(
            "  {:<3} {:<30} {:<20} {}",
            token.id.to_string().bright_black(),
            token.name.cyan(),
            token
                .created_at
                .format("%Y-%m-%d %H:%M")
                .to_string()
                .bright_black(),
            status
        );
    }

    println!();
    println!(
        "  Total: {}",
        tokens.len().to_string().bright_white().bold()
    );
    println!();

    Ok(())
}

/// Revokes a token by name or ID with confirmation prompt.
///
/// # Lookup
///
/// - If input is numeric, lookup by ID
/// - Otherwise, lookup by name (exact match)
///
/// # Safety
///
/// - Requires confirmation (default: No)
/// - Prevents double-revocation
async fn revoke_token(repo: Arc<PgTokenRepository>, name_or_hash: String) -> Result<()> {
    println!("{}", "🔒 Revoke API Token".bright_blue().bold());
    println!();

    // Try to find by name or ID
    let token = match name_or_hash.parse::<i64>() {
        Ok(id) => repo
            .find_by_id(id)
            .await
            .map_err(|e| anyhow::anyhow!("Database error: {}", e))?,
        Err(_) => repo
            .find_by_name(&name_or_hash)
            .await
            .map_err(|e| anyhow::anyhow!("Database error: {}", e))?,
    };

    let token = token.context("Token not found")?;

    if token.revoked_at.is_some() {
        println!("{}", "⚠️  This token is already revoked".yellow());
        return Ok(());
    }

    println!("  Token: {}", token.name.cyan());
    println!("  ID:    {}", token.id.to_string().bright_black());
    println!();

    let confirmed = Confirm::new()
        .with_prompt("Revoke this token?")
        .default(false)
        .interact()?;

    if !confirmed {
        println!("{}", "❌ Cancelled".red());
        return Ok(());
    }

    repo.revoke_token(token.id)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to revoke token: {}", e))?;

    println!();
    println!("{}", "✅ Token revoked successfully!".green().bold());
    println!();

    Ok(())
}

/// Displays system statistics.
///
/// Shows:
/// - Total number of links
/// - Number of active API tokens
///
/// Click totals are no longer sourced from PostgreSQL: clicks now live in
/// ClickHouse (the `link_clicks` table was dropped), which this admin tool does
/// not connect to. Use the `/api/stats` endpoint for click analytics.
async fn handle_stats(pool: &PgPool) -> Result<()> {
    println!("{}", "📊 Statistics".bright_blue().bold());
    println!();

    let links_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM links")
        .fetch_one(pool)
        .await?;

    let tokens_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM api_tokens WHERE revoked_at IS NULL")
            .fetch_one(pool)
            .await?;

    println!(
        "  Links:         {}",
        links_count.to_string().bright_green().bold()
    );
    println!(
        "  Active tokens: {}",
        tokens_count.to_string().bright_green().bold()
    );
    println!(
        "  Clicks:        {}",
        "see /api/stats (ClickHouse)".dimmed()
    );
    println!();

    Ok(())
}

/// Handles database diagnostic commands.
async fn handle_db_action(action: DbAction, pool: &PgPool) -> Result<()> {
    match action {
        DbAction::Check => {
            println!("{}", "🔍 Checking database connection...".bright_blue());

            sqlx::query("SELECT 1").fetch_one(pool).await?;

            println!("{}", "✅ Database connection OK".green().bold());
        }
        DbAction::Info => {
            println!("{}", "ℹ️  Database Information".bright_blue().bold());
            println!();

            let version: String = sqlx::query_scalar("SELECT version()")
                .fetch_one(pool)
                .await?;

            println!("  PostgreSQL: {}", version.bright_white());
            println!();
        }
    }

    Ok(())
}

/// Generates a cryptographically random token.
///
/// # Format
///
/// - Length: 48 characters
/// - Character set: A-Z, a-z, 0-9
/// - Entropy: ~286 bits
fn generate_token() -> String {
    use rand::Rng;
    const CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    const TOKEN_LEN: usize = 48;

    let mut rng = rand::rng();

    (0..TOKEN_LEN)
        .map(|_| {
            let idx = rng.random_range(0..CHARSET.len());
            CHARSET[idx] as char
        })
        .collect()
}

/// Hashes a token with HMAC-SHA256 keyed by `secret`.
///
/// Returns a 64-character lowercase hex-encoded MAC for database storage.
fn hash_token(token: &str, secret: &str) -> String {
    let mut mac =
        HmacSha256::new_from_slice(secret.as_bytes()).expect("HMAC accepts any key length");
    mac.update(token.as_bytes());
    hex::encode(mac.finalize().into_bytes())
}
