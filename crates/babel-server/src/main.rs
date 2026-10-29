use std::io::IsTerminal;
use std::path::PathBuf;

use babel_core::Store;
use babel_server::{AppState, Mode, router};
use clap::{Parser, Subcommand};

/// Babel's server: keeps translations for signed-in users, serves the web
/// app, and is what the desktop app syncs with.
#[derive(Parser)]
#[command(version)]
struct Cli {
    /// The database file.
    #[arg(long, env = "BABEL_DB", default_value = "babel.db", global = true)]
    db: PathBuf,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Serve the API and the web app.
    Serve {
        #[arg(long, env = "BABEL_LISTEN", default_value = "127.0.0.1:8790")]
        listen: String,
        /// Mark the session cookie Secure (set this behind HTTPS).
        #[arg(long, env = "BABEL_SECURE_COOKIES")]
        secure_cookies: bool,
    },
    /// Manage accounts.
    #[command(subcommand)]
    User(UserCmd),
}

#[derive(Subcommand)]
enum UserCmd {
    /// Create an account. The password is asked for, or read from stdin.
    Add { name: String },
    /// Set a new password; signs the user out everywhere.
    Passwd { name: String },
    /// Delete an account and all its translations.
    Del { name: String },
    /// List accounts.
    List,
}

fn password() -> anyhow::Result<String> {
    if !std::io::stdin().is_terminal() {
        let mut s = String::new();
        std::io::stdin().read_line(&mut s)?;
        return Ok(s.trim_end_matches(['\r', '\n']).to_owned());
    }
    let p = rpassword::prompt_password("Password: ")?;
    if rpassword::prompt_password("Again: ")? != p {
        anyhow::bail!("the passwords differ");
    }
    Ok(p)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,tower_http=warn".into()),
        )
        .init();
    let cli = Cli::parse();
    let store = Store::open(&cli.db)?;
    match cli.cmd {
        Cmd::Serve {
            listen,
            secure_cookies,
        } => {
            let mut state = AppState::new(store, Mode::Server);
            state.secure_cookies = secure_cookies;
            let listener = tokio::net::TcpListener::bind(&listen).await?;
            tracing::info!(
                "Babel on http://{} ({})",
                listener.local_addr()?,
                cli.db.display()
            );
            axum::serve(listener, router(state))
                .with_graceful_shutdown(async {
                    let _ = tokio::signal::ctrl_c().await;
                })
                .await?;
        }
        Cmd::User(UserCmd::Add { name }) => {
            let u = store.create_user(&name, &password()?)?;
            println!("Created {}.", u.name);
        }
        Cmd::User(UserCmd::Passwd { name }) => {
            store.set_password(&name, &password()?)?;
            println!("Password changed; {name} is signed out everywhere.");
        }
        Cmd::User(UserCmd::Del { name }) => {
            store.delete_user(&name)?;
            println!("Deleted {name}.");
        }
        Cmd::User(UserCmd::List) => {
            for u in store.users()? {
                println!("{}", u.name);
            }
        }
    }
    Ok(())
}
