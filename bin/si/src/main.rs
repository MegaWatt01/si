use crate::args::{Commands, Engine};
use color_eyre::Result;
use si_cli::cmd::{check, delete, install, launch, report, restart, start, status, stop, update};
use telemetry_application::{prelude::*, TelemetryConfig};
use tokio::sync::oneshot::Sender;

mod args;

#[tokio::main(flavor = "multi_thread", worker_threads = 10)]
async fn main() -> Result<()> {
    color_eyre::install()?;
    let config = TelemetryConfig::builder()
        .service_name("cli")
        .service_namespace("cli")
        .log_env_var_prefix("SI")
        .app_modules(vec!["si"])
        .build()?;
    let _telemetry = telemetry_application::init(config)?;
    let args = args::parse();
    let mode = args.mode();

    debug!(arguments =?args, "parsed cli arguments");

    let (ph_client, ph_sender) = si_posthog::new().request_timeout_ms(3000).build()?;
    let (ph_done_sender, ph_done_receiver) = tokio::sync::oneshot::channel();

    tokio::spawn(wait_for_posthog_flush(ph_done_sender, ph_sender));

    println!(
        "{}\n\n",
        format_args!(
            "System Initiative Launcher is in {:?} mode",
            mode.to_string()
        )
    );

    if !matches!(args.command, Commands::Update(_)) {
        match update::find().await {
            Ok(Some(_)) => {
                println!("Update found, please run `si update` to install it\n");
            }
            Ok(None) => {}
            Err(err) => {
                println!("Unable to retrieve updates: {err}");
            }
        }
    }

    if let Engine::Podman = args.engine() {
        println!("Podman isn't supported as an engine at this time! It's coming soon though...");
        return Ok(());
    }

    match args.command {
        Commands::Install(_args) => {
            install::invoke(&ph_client, mode.to_string()).await?;
        }
        Commands::Check(_args) => {
            check::invoke(&ph_client, mode.to_string(), false).await?;
        }
        Commands::Launch(_args) => {
            launch::invoke(&ph_client, mode.to_string())?;
        }
        Commands::Start(_args) => {
            start::invoke(&ph_client, mode.to_string()).await?;
        }
        Commands::Delete(_args) => {
            delete::invoke(&ph_client, mode.to_string()).await?;
        }
        Commands::Restart(_args) => {
            restart::invoke(&ph_client, mode.to_string())?;
        }
        Commands::Stop(_args) => {
            stop::invoke(&ph_client, mode.to_string()).await?;
        }
        Commands::Update(args) => {
            update::invoke(&ph_client, mode.to_string(), args.skip_check).await?;
        }
        Commands::Status(_args) => {
            status::invoke(&ph_client, mode.to_string()).await?;
        }
        Commands::Report(_args) => {
            report::invoke(&ph_client, mode.to_string())?;
        }
    }
    drop(ph_client);

    if let Err(e) = ph_done_receiver.await {
        println!("{}", e)
    }
    Ok(())
}

async fn wait_for_posthog_flush(done_sender: Sender<()>, sender: si_posthog::PosthogSender) {
    sender.run().await;
    done_sender
        .send(())
        .expect("Unable to push events to Posthog")
}
