use url_shortener::domain::repositories::TokenRepository;
use url_shortener::infrastructure::persistence::PgTokenRepository;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use colored::*;
use dialoguer::{Confirm, Input};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use std::sync::Arc;

/// CLI tool for managing url-shortener
#[derive(Parser)]
#[command(name = "admin")]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

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
        /// Token name or hash to revoke
        name_or_hash: String,
    },
}

#[derive(Subcommand)]
enum DbAction {
    /// Check database connection
    Check,

    /// Show database info
    Info,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Load configuration
    dotenvy::dotenv().ok();

    let cli = Cli::parse();

    // Connect to database
    let database_url = std::env::var("DATABASE_URL").context("DATABASE_URL must be set")?;

    let pool = PgPool::connect(&database_url)
        .await
        .context("Failed to connect to database")?;

    match cli.command {
        Commands::Token { action } => handle_token_action(action, &pool).await?,
        Commands::Stats => handle_stats(&pool).await?,
        Commands::Db { action } => handle_db_action(action, &pool).await?,
    }

    Ok(())
}

async fn handle_token_action(action: TokenAction, pool: &PgPool) -> Result<()> {
    let repo = Arc::new(PgTokenRepository::new(Arc::new(pool.clone())));

    match action {
        TokenAction::Create { name, token, yes } => {
            create_token(repo, name, token, yes).await?;
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

async fn create_token(
    repo: Arc<PgTokenRepository>,
    name: Option<String>,
    token: Option<String>,
    skip_confirm: bool,
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
    let token_hash = hash_token(&token_value);

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
        "  curl -H \"Authorization: Bearer {}\" http://localhost:3000/stats",
        token_value.bright_yellow()
    );
    println!();

    Ok(())
}

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

async fn handle_stats(pool: &PgPool) -> Result<()> {
    println!("{}", "📊 Statistics".bright_blue().bold());
    println!();

    let links_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM links")
        .fetch_one(pool)
        .await?;

    let clicks_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM link_clicks")
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
        "  Clicks:        {}",
        clicks_count.to_string().bright_green().bold()
    );
    println!(
        "  Active tokens: {}",
        tokens_count.to_string().bright_green().bold()
    );
    println!();

    Ok(())
}

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

fn generate_token() -> String {
    use rand::Rng;
    const CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    const TOKEN_LEN: usize = 48;

    let mut rng = rand::rng(); // ИСПРАВЛЕНО: используем rand::rng() вместо thread_rng()

    (0..TOKEN_LEN)
        .map(|_| {
            let idx = rng.random_range(0..CHARSET.len()); // ИСПРАВЛЕНО: random_range вместо gen_range
            CHARSET[idx] as char
        })
        .collect()
}

fn hash_token(token: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    format!("{:x}", hasher.finalize()) // ИСПРАВЛЕНО: используем format! вместо hex::encode
}
